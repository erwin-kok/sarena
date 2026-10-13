use std::time::Duration;

use sarena_data_plane::{Cancelled, EventQueue, Handler, Pending, Stopped, TryEnqueueError};
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};

#[derive(Debug)]
enum Ev {
    Add(u32, u32),
    /// Sleeps, then appends `id` to the handler's log.
    Record {
        id: u32,
        delay: Duration,
    },
    /// Blocks the queue until the sender fires.
    Block(oneshot::Receiver<()>),
    /// Returns the handler's log so far.
    Log,
    Panic,
}

impl PartialEq for Ev {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (Ev::Add(a, b), Ev::Add(c, d)) if (a, b) == (c, d))
    }
}

#[derive(Default)]
struct TestHandler {
    log: Vec<u32>,
}

impl Handler for TestHandler {
    type Event = Ev;
    type Output = Vec<u32>;

    async fn handle(&mut self, event: Ev) -> Vec<u32> {
        match event {
            Ev::Add(a, b) => vec![a + b],
            Ev::Record { id, delay } => {
                sleep(delay).await;
                self.log.push(id);
                vec![]
            }
            Ev::Block(rx) => {
                let _ = rx.await;
                vec![]
            }
            Ev::Log => self.log.clone(),
            Ev::Panic => panic!("handler failed"),
        }
    }
}

fn queue(capacity: usize) -> EventQueue<TestHandler> {
    EventQueue::new("q", capacity, TestHandler::default())
}

/// Enqueues an event that blocks the queue until the returned sender fires.
async fn block(q: &EventQueue<TestHandler>) -> (oneshot::Sender<()>, Pending<Vec<u32>>) {
    let (tx, rx) = oneshot::channel();
    let pending = q.enqueue(Ev::Block(rx)).await.unwrap();
    sleep(Duration::from_millis(1)).await; // let the worker pick it up
    (tx, pending)
}

#[tokio::test(start_paused = true)]
async fn returns_handler_results() {
    let q = queue(4);
    let pending = q.enqueue(Ev::Add(2, 3)).await.unwrap();
    assert_eq!(pending.await, Ok(vec![5]));
}

#[tokio::test(start_paused = true)]
async fn runs_in_fifo_order_with_exclusive_state() {
    let q = queue(16);
    let mut pendings = Vec::new();
    for id in 0..5u32 {
        // Earlier events take longer; they must still finish first.
        let delay = Duration::from_millis(50 * u64::from(5 - id));
        pendings.push(q.enqueue(Ev::Record { id, delay }).await.unwrap());
    }
    for pending in pendings {
        pending.await.unwrap();
    }
    assert_eq!(
        q.enqueue(Ev::Log).await.unwrap().await,
        Ok(vec![0, 1, 2, 3, 4])
    );
}

#[tokio::test(start_paused = true)]
async fn stop_finishes_the_running_event_and_cancels_the_rest() {
    let q = queue(8);
    let (release, running) = block(&q).await;
    let queued1 = q.enqueue(Ev::Add(1, 1)).await.unwrap();
    let queued2 = q.enqueue(Ev::Add(2, 2)).await.unwrap();

    let termination = q.stop();
    assert!(q.is_stopped());
    release.send(()).unwrap();

    assert_eq!(running.await, Ok(vec![]));
    assert_eq!(queued1.await, Err(Cancelled));
    assert_eq!(queued2.await, Err(Cancelled));
    timeout(Duration::from_secs(1), termination)
        .await
        .expect("drained");
    assert_eq!(q.enqueue(Ev::Add(3, 3)).await.err(), Some(Stopped));
    timeout(Duration::from_secs(1), q.stop())
        .await
        .expect("stop is idempotent");
}

#[tokio::test(start_paused = true)]
async fn try_enqueue_hands_the_event_back_when_full() {
    let q = queue(1);
    let (release, _running) = block(&q).await;
    let queued = q.try_enqueue(Ev::Add(1, 1)).unwrap(); // fills the one buffer slot

    match q.try_enqueue(Ev::Add(2, 2)) {
        Err(TryEnqueueError::Full(event)) => assert_eq!(event, Ev::Add(2, 2)),
        other => panic!("expected Full, got {other:?}"),
    }
    release.send(()).unwrap();
    assert_eq!(queued.await, Ok(vec![2]));

    q.stop().await;
    let err = q.try_enqueue(Ev::Add(3, 3)).unwrap_err();
    assert!(matches!(err, TryEnqueueError::Stopped(_)));
    assert_eq!(err.into_inner(), Ev::Add(3, 3));
}

#[tokio::test(start_paused = true)]
async fn enqueue_waits_while_full() {
    let q = queue(1);
    let (release, _running) = block(&q).await;
    let _queued = q.enqueue(Ev::Add(1, 1)).await.unwrap();

    let q2 = q.clone();
    let waiter = tokio::spawn(async move { q2.enqueue(Ev::Add(5, 5)).await.unwrap().await });
    sleep(Duration::from_secs(10)).await;
    assert!(!waiter.is_finished(), "enqueue applies backpressure");

    release.send(()).unwrap();
    assert_eq!(waiter.await.unwrap(), Ok(vec![10]));
}

#[tokio::test(start_paused = true)]
async fn a_panicking_handler_stops_the_queue() {
    let q = queue(4);
    let (release, _running) = block(&q).await;
    let bad = q.enqueue(Ev::Panic).await.unwrap();
    let after = q.enqueue(Ev::Add(1, 2)).await.unwrap();
    release.send(()).unwrap();

    assert_eq!(bad.await, Err(Cancelled));
    assert_eq!(after.await, Err(Cancelled));
    timeout(Duration::from_secs(1), q.stop())
        .await
        .expect("drained");
    assert!(q.is_stopped());
    assert_eq!(q.enqueue(Ev::Add(1, 2)).await.err(), Some(Stopped));
}

#[tokio::test(start_paused = true)]
async fn dropping_the_last_handle_stops_the_queue() {
    let q = queue(4);
    let (release, running) = block(&q).await;
    let queued = q.enqueue(Ev::Add(1, 1)).await.unwrap();

    let clone = q.clone();
    drop(q);
    assert!(
        !clone.is_stopped(),
        "a remaining handle keeps the queue alive"
    );
    drop(clone);

    release.send(()).unwrap();
    assert_eq!(running.await, Ok(vec![]));
    assert_eq!(queued.await, Err(Cancelled));
}
