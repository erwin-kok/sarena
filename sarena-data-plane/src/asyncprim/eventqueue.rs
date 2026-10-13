use std::{
    fmt,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::FutureExt;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::asyncprim::{Stopped, Termination, util::panic_message};

/// Owns the state an [`EventQueue`] protects and handles its events, one at a
/// time. The queue's worker task owns the handler, so `handle` gets exclusive
/// access to it without any locking.
pub trait Handler: Send + 'static {
    type Event: Send + 'static;
    type Output: Send + 'static;

    fn handle(&mut self, event: Self::Event) -> impl Future<Output = Self::Output> + Send;

    /// Names an event in the queue's tracing output.
    fn event_name(_event: &Self::Event) -> &'static str {
        std::any::type_name::<Self::Event>()
    }
}

/// The queue stopped before the event ran, or the event's handler panicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("event cancelled")]
pub struct Cancelled;

/// Why [`EventQueue::try_enqueue`] failed. The event is handed back.
#[derive(Debug)]
pub enum TryEnqueueError<E> {
    Full(E),
    Stopped(E),
}

impl<E> TryEnqueueError<E> {
    pub fn into_inner(self) -> E {
        match self {
            TryEnqueueError::Full(e) | TryEnqueueError::Stopped(e) => e,
        }
    }
}

impl<E> fmt::Display for TryEnqueueError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TryEnqueueError::Full(_) => f.write_str("event queue is full"),
            TryEnqueueError::Stopped(_) => f.write_str("event queue is stopped"),
        }
    }
}

impl<E: fmt::Debug> std::error::Error for TryEnqueueError<E> {}

/// The result of an enqueued event. Resolves to `Err(Cancelled)` if the event
/// never ran to completion.
#[derive(Debug)]
pub struct Pending<T>(oneshot::Receiver<T>);

impl<T> Future for Pending<T> {
    type Output = Result<T, Cancelled>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx).map_err(|_| Cancelled)
    }
}

struct Job<H: Handler> {
    event: H::Event,
    reply: oneshot::Sender<H::Output>,
    queued_at: Instant,
}

struct Inner<H: Handler> {
    name: Arc<str>,
    tx: mpsc::Sender<Job<H>>,
    stop: CancellationToken,
    done: CancellationToken,
}

impl<H: Handler> Drop for Inner<H> {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// Hands events to a [`Handler`] one at a time, in the order they were
/// enqueued.
///
/// Cloning gives another handle to the same queue. When the last handle is
/// dropped, the queue stops: the running event finishes and the rest are
/// cancelled. If the handler panics, the queue stops too, since its state may
/// be left half-updated. The handler is dropped once the queue has drained.
pub struct EventQueue<H: Handler> {
    inner: Arc<Inner<H>>,
}

impl<H: Handler> Clone for EventQueue<H> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<H: Handler> EventQueue<H> {
    /// Creates a queue that buffers up to `capacity` events (at least 1) and
    /// moves `handler` into its worker task.
    pub fn new(name: impl Into<String>, capacity: usize, handler: H) -> Self {
        let name: Arc<str> = name.into().into();
        let (tx, rx) = mpsc::channel(capacity.max(1));
        let stop = CancellationToken::new();
        let done = CancellationToken::new();
        let guard = done.clone().drop_guard();

        tokio::spawn(worker(name.clone(), handler, rx, stop.clone(), guard));

        Self {
            inner: Arc::new(Inner {
                name,
                tx,
                stop,
                done,
            }),
        }
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Adds an event to the queue, waiting while the queue is full. Returns
    /// once the event is queued; await the returned [`Pending`] for its result.
    pub async fn enqueue(&self, event: H::Event) -> Result<Pending<H::Output>, Stopped> {
        if self.inner.stop.is_cancelled() {
            return Err(Stopped);
        }
        let permit = tokio::select! {
            biased;
            () = self.inner.stop.cancelled() => return Err(Stopped),
            permit = self.inner.tx.reserve() => permit.map_err(|_| Stopped)?,
        };
        let (job, pending) = package(event);
        permit.send(job);
        Ok(pending)
    }

    /// Adds an event without waiting. On failure the event is handed back.
    pub fn try_enqueue(
        &self,
        event: H::Event,
    ) -> Result<Pending<H::Output>, TryEnqueueError<H::Event>> {
        if self.inner.stop.is_cancelled() {
            return Err(TryEnqueueError::Stopped(event));
        }
        match self.inner.tx.try_reserve() {
            Ok(permit) => {
                let (job, pending) = package(event);
                permit.send(job);
                Ok(pending)
            }
            Err(mpsc::error::TrySendError::Full(())) => Err(TryEnqueueError::Full(event)),
            Err(mpsc::error::TrySendError::Closed(())) => Err(TryEnqueueError::Stopped(event)),
        }
    }

    /// Stops the queue: the running event finishes, queued events are
    /// cancelled, and new events are refused. Await the result to wait until
    /// the queue is drained. Calling it more than once is safe.
    pub fn stop(&self) -> Termination {
        self.inner.stop.cancel();
        Termination::from_token(self.inner.done.clone())
    }

    pub fn is_stopped(&self) -> bool {
        self.inner.stop.is_cancelled()
    }
}

impl<H: Handler> fmt::Debug for EventQueue<H> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventQueue")
            .field("name", &self.inner.name)
            .field("stopped", &self.is_stopped())
            .finish()
    }
}

fn package<H: Handler>(event: H::Event) -> (Job<H>, Pending<H::Output>) {
    let (reply, rx) = oneshot::channel();
    let job = Job {
        event,
        reply,
        queued_at: Instant::now(),
    };
    (job, Pending(rx))
}

async fn worker<H: Handler>(
    name: Arc<str>,
    mut handler: H,
    mut rx: mpsc::Receiver<Job<H>>,
    stop: CancellationToken,
    _done: DropGuard,
) {
    loop {
        let job = tokio::select! {
            biased;
            () = stop.cancelled() => break,
            job = rx.recv() => match job {
                Some(job) => job,
                None => break,
            },
        };
        let kind = H::event_name(&job.event);
        let waited = job.queued_at.elapsed();
        let started = Instant::now();
        // The running event is awaited outside the select, so stopping the
        // queue never interrupts it.
        match AssertUnwindSafe(handler.handle(job.event))
            .catch_unwind()
            .await
        {
            Ok(output) => {
                let _ = job.reply.send(output);
            }
            Err(payload) => {
                // The handler may have left its state half-updated; don't hand
                // it any more events. Dropping the reply cancels this one.
                tracing::error!(queue = %name, event = kind, "event handler {}, stopping queue", panic_message(payload));
                stop.cancel();
                break;
            }
        }
        tracing::debug!(queue = %name, event = kind, ?waited, took = ?started.elapsed(), "event processed");
    }

    // Dropping the queued jobs drops their result senders, which resolves
    // every waiting Pending to Err(Cancelled).
    rx.close();
    while rx.try_recv().is_ok() {}
}
