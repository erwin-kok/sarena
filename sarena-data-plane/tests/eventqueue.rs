use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::Duration,
};

use sarena_data_plane::{Cancelled, Event, EventQueue, Stopped, TryEnqueueError};
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};

#[derive(Debug, PartialEq)]
struct Add(u32, u32);

impl Event for Add {
    type Output = u32;

    async fn handle(self) -> u32 {
        self.0 + self.1
    }
}

/// Enqueues an event that blocks the queue until the returned sender fires.
async fn block(
    q: &EventQueue,
) -> (
    oneshot::Sender<()>,
    sarena_data_plane::Pending<&'static str>,
) {
    let (tx, rx) = oneshot::channel::<()>();
    let pending = q
        .run(move || async move {
            let _ = rx.await;
            "released"
        })
        .await
        .unwrap();
    sleep(Duration::from_millis(1)).await; // let the worker pick it up
    (tx, pending)
}

#[tokio::test(start_paused = true)]
async fn returns_typed_results() {
    let q = EventQueue::new("q", 4);
    let pending = q.enqueue(Add(2, 3)).await.unwrap();
    assert_eq!(pending.await, Ok(5));
}

#[tokio::test(start_paused = true)]
async fn runs_in_fifo_order_one_at_a_time() {
    let q = EventQueue::new("q", 16);
    let log = Arc::new(Mutex::new(Vec::new()));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));

    let mut pendings = Vec::new();
    for i in 0..5u64 {
        let (log, in_flight, max_in_flight) =
            (log.clone(), in_flight.clone(), max_in_flight.clone());
        let pending = q
            .run(move || async move {
                let now = in_flight.fetch_add(1, SeqCst) + 1;
                max_in_flight.fetch_max(now, SeqCst);
                sleep(Duration::from_millis(50 * (5 - i))).await; // earlier events take longer
                log.lock().unwrap().push(i);
                in_flight.fetch_sub(1, SeqCst);
            })
            .await
            .unwrap();
        pendings.push(pending);
    }
    for pending in pendings {
        pending.await.unwrap();
    }
    assert_eq!(*log.lock().unwrap(), [0, 1, 2, 3, 4]);
    assert_eq!(max_in_flight.load(SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn stop_finishes_the_running_event_and_cancels_the_rest() {
    let q = EventQueue::new("q", 8);
    let (release, running) = block(&q).await;
    let queued1 = q.enqueue(Add(1, 1)).await.unwrap();
    let queued2 = q.enqueue(Add(2, 2)).await.unwrap();

    let termination = q.stop();
    assert!(q.is_stopped());
    release.send(()).unwrap();

    assert_eq!(running.await, Ok("released"));
    assert_eq!(queued1.await, Err(Cancelled));
    assert_eq!(queued2.await, Err(Cancelled));
    timeout(Duration::from_secs(1), termination)
        .await
        .expect("drained");
    assert_eq!(q.enqueue(Add(3, 3)).await.err(), Some(Stopped));
    timeout(Duration::from_secs(1), q.stop())
        .await
        .expect("stop is idempotent");
}

#[tokio::test(start_paused = true)]
async fn try_enqueue_hands_the_event_back_when_full() {
    let q = EventQueue::new("q", 1);
    let (release, _running) = block(&q).await;
    let queued = q.try_enqueue(Add(1, 1)).unwrap(); // fills the one buffer slot

    match q.try_enqueue(Add(2, 2)) {
        Err(TryEnqueueError::Full(event)) => assert_eq!(event, Add(2, 2)),
        other => panic!("expected Full, got {other:?}"),
    }
    release.send(()).unwrap();
    assert_eq!(queued.await, Ok(2));

    q.stop().await;
    let err = q.try_enqueue(Add(3, 3)).unwrap_err();
    assert!(matches!(err, TryEnqueueError::Stopped(_)));
    assert_eq!(err.into_inner(), Add(3, 3));
}

#[tokio::test(start_paused = true)]
async fn enqueue_waits_while_full() {
    let q = EventQueue::new("q", 1);
    let (release, _running) = block(&q).await;
    let _queued = q.enqueue(Add(1, 1)).await.unwrap();

    let q2 = q.clone();
    let waiter = tokio::spawn(async move { q2.enqueue(Add(5, 5)).await.unwrap().await });
    sleep(Duration::from_secs(10)).await;
    assert!(!waiter.is_finished(), "enqueue applies backpressure");

    release.send(()).unwrap();
    assert_eq!(waiter.await.unwrap(), Ok(10));
}

#[tokio::test(start_paused = true)]
async fn a_panicking_handler_cancels_only_its_own_event() {
    let q = EventQueue::new("q", 4);
    let bad = q
        .run(|| async {
            if true {
                panic!("handler failed");
            }
            0u32
        })
        .await
        .unwrap();
    assert_eq!(bad.await, Err(Cancelled));
    assert_eq!(q.enqueue(Add(1, 2)).await.unwrap().await, Ok(3));
}

#[tokio::test(start_paused = true)]
async fn dropping_the_last_handle_stops_the_queue() {
    let q = EventQueue::new("q", 4);
    let (release, running) = block(&q).await;
    let queued = q.enqueue(Add(1, 1)).await.unwrap();

    let clone = q.clone();
    drop(q);
    assert!(
        !clone.is_stopped(),
        "a remaining handle keeps the queue alive"
    );
    drop(clone);

    release.send(()).unwrap();
    assert_eq!(running.await, Ok("released"));
    assert_eq!(queued.await, Err(Cancelled));
}
