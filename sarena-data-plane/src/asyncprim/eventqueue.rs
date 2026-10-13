use std::{
    fmt,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::{FutureExt, future::BoxFuture};
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::asyncprim::{Stopped, Termination, util::panic_message};

pub trait Event: Send + 'static {
    type Output: Send + 'static;

    fn handle(self) -> impl Future<Output = Self::Output> + Send;
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

struct Job {
    run: BoxFuture<'static, ()>,
    queued_at: Instant,
    kind: &'static str,
}

struct Inner {
    name: Arc<str>,
    tx: mpsc::Sender<Job>,
    stop: CancellationToken,
    done: CancellationToken,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// Runs events one at a time, in the order they were enqueued.
///
/// Cloning gives another handle to the same queue. When the last handle is
/// dropped, the queue stops: the running event finishes and the rest are
/// cancelled.
#[derive(Clone)]
pub struct EventQueue {
    inner: Arc<Inner>,
}

impl EventQueue {
    /// Creates a queue that buffers up to `capacity` events (at least 1).
    pub fn new(name: impl Into<String>, capacity: usize) -> Self {
        let name: Arc<str> = name.into().into();
        let (tx, rx) = mpsc::channel(capacity.max(1));
        let stop = CancellationToken::new();
        let done = CancellationToken::new();
        let guard = done.clone().drop_guard();

        tokio::spawn(worker(name.clone(), rx, stop.clone(), guard));

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
    pub async fn enqueue<E: Event>(&self, event: E) -> Result<Pending<E::Output>, Stopped> {
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
    pub fn try_enqueue<E: Event>(
        &self,
        event: E,
    ) -> Result<Pending<E::Output>, TryEnqueueError<E>> {
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

    /// Enqueues an async closure as an event.
    pub async fn run<F, Fut, T>(&self, f: F) -> Result<Pending<T>, Stopped>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.enqueue(FnEvent(f)).await
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

impl fmt::Debug for EventQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventQueue")
            .field("name", &self.inner.name)
            .field("stopped", &self.is_stopped())
            .finish()
    }
}

struct FnEvent<F>(F);

impl<F, Fut, T> Event for FnEvent<F>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    type Output = T;

    fn handle(self) -> impl Future<Output = T> + Send {
        (self.0)()
    }
}

fn package<E: Event>(event: E) -> (Job, Pending<E::Output>) {
    let (tx, rx) = oneshot::channel();
    let run = Box::pin(async move {
        let output = event.handle().await;
        let _ = tx.send(output);
    });
    let job = Job {
        run,
        queued_at: Instant::now(),
        kind: std::any::type_name::<E>(),
    };
    (job, Pending(rx))
}

async fn worker(
    name: Arc<str>,
    mut rx: mpsc::Receiver<Job>,
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
        let waited = job.queued_at.elapsed();
        let started = Instant::now();
        // The running event is awaited outside the select, so stopping the
        // queue never interrupts it.
        if let Err(payload) = AssertUnwindSafe(job.run).catch_unwind().await {
            tracing::error!(queue = %name, event = job.kind, "event handler {}", panic_message(payload));
        }
        tracing::debug!(queue = %name, event = job.kind, ?waited, took = ?started.elapsed(), "event processed");
    }
    
    // Dropping the queued jobs drops their result senders, which resolves
    // every waiting Pending to Err(Cancelled).
    rx.close();
    while rx.try_recv().is_ok() {}
}
