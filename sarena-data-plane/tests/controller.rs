use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::Duration,
};

use sarena_data_plane::{Backoff, Flow, JobFn, Manager, Registry, Spec, job, stop_fn};
use tokio::time::{Instant, sleep, timeout};

fn manager() -> Manager {
    Manager::builder().registry(Registry::new()).build()
}

fn counting(count: &Arc<AtomicUsize>, flow: Flow) -> JobFn {
    let count = count.clone();
    job(move |_| {
        let count = count.clone();
        async move {
            count.fetch_add(1, SeqCst);
            Ok(flow)
        }
    })
}

fn failing(times: &Arc<Mutex<Vec<Instant>>>) -> JobFn {
    let times = times.clone();
    job(move |_| {
        let times = times.clone();
        async move {
            times.lock().unwrap().push(Instant::now());
            Err("boom".into())
        }
    })
}

fn gaps(times: &Arc<Mutex<Vec<Instant>>>) -> Vec<u64> {
    let t = times.lock().unwrap();
    t.windows(2).map(|w| (w[1] - w[0]).as_secs()).collect()
}

#[test]
fn backoff_delays() {
    let s = Duration::from_secs;
    let linear = Backoff::Linear {
        base: s(1),
        max: s(3),
    };
    let expo = Backoff::Exponential {
        base: s(1),
        max: s(5),
    };
    assert_eq!(
        (1..=5)
            .map(|n| linear.delay(n).unwrap().as_secs())
            .collect::<Vec<_>>(),
        [1, 2, 3, 3, 3]
    );
    assert_eq!(
        (1..=5)
            .map(|n| expo.delay(n).unwrap().as_secs())
            .collect::<Vec<_>>(),
        [1, 2, 4, 5, 5]
    );
    assert_eq!(Backoff::None.delay(3), None);
    assert_eq!(expo.delay(u32::MAX), Some(s(5)));
}

#[tokio::test(start_paused = true)]
async fn runs_once_without_interval_and_reruns_on_update() {
    let m = manager();
    let n = Arc::new(AtomicUsize::new(0));
    m.update("a", Spec::new(counting(&n, Flow::Continue)));
    sleep(Duration::from_secs(3600)).await;
    assert_eq!(n.load(SeqCst), 1);

    m.update("a", Spec::new(counting(&n, Flow::Continue)));
    sleep(Duration::from_millis(1)).await;
    assert_eq!(n.load(SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn repeats_on_interval() {
    let m = manager();
    let n = Arc::new(AtomicUsize::new(0));
    m.update(
        "a",
        Spec::new(counting(&n, Flow::Continue)).interval(Duration::from_secs(10)),
    );
    sleep(Duration::from_secs(35)).await;
    assert_eq!(n.load(SeqCst), 4); // t = 0, 10, 20, 30
}

#[tokio::test(start_paused = true)]
async fn done_parks_even_with_interval() {
    let m = manager();
    let n = Arc::new(AtomicUsize::new(0));
    m.update(
        "a",
        Spec::new(counting(&n, Flow::Done)).interval(Duration::from_secs(10)),
    );
    sleep(Duration::from_secs(100)).await;
    assert_eq!(n.load(SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn exponential_backoff_is_capped() {
    let m = manager();
    let times = Arc::new(Mutex::new(Vec::new()));
    let backoff = Backoff::Exponential {
        base: Duration::from_secs(1),
        max: Duration::from_secs(4),
    };
    m.update("a", Spec::new(failing(&times)).backoff(backoff));
    sleep(Duration::from_millis(11_500)).await;
    assert_eq!(gaps(&times), [1, 2, 4, 4]); // t = 0, 1, 3, 7, 11
}

#[tokio::test(start_paused = true)]
async fn no_backoff_and_no_interval_parks_instead_of_spinning() {
    let m = manager();
    let times = Arc::new(Mutex::new(Vec::new()));
    m.update("a", Spec::new(failing(&times)).backoff(Backoff::None));
    sleep(Duration::from_secs(3600)).await;
    assert_eq!(times.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn no_backoff_retries_at_the_interval() {
    let m = manager();
    let times = Arc::new(Mutex::new(Vec::new()));
    m.update(
        "a",
        Spec::new(failing(&times))
            .backoff(Backoff::None)
            .interval(Duration::from_secs(5)),
    );
    sleep(Duration::from_secs(11)).await;
    assert_eq!(gaps(&times), [5, 5]);
}

#[tokio::test(start_paused = true)]
async fn update_interrupts_backoff() {
    let m = manager();
    let times = Arc::new(Mutex::new(Vec::new()));
    let slow = Backoff::Exponential {
        base: Duration::from_secs(60),
        max: Duration::from_secs(60),
    };
    m.update("a", Spec::new(failing(&times)).backoff(slow));
    sleep(Duration::from_secs(1)).await;

    let n = Arc::new(AtomicUsize::new(0));
    m.update("a", Spec::new(counting(&n, Flow::Done)));
    sleep(Duration::from_millis(1)).await;
    assert_eq!(n.load(SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn updates_during_a_run_fold_into_one_rerun() {
    let m = manager();
    let n = Arc::new(AtomicUsize::new(0));
    let slow = {
        let n = n.clone();
        job(move |_| {
            let n = n.clone();
            async move {
                n.fetch_add(1, SeqCst);
                sleep(Duration::from_secs(5)).await;
                Ok(Flow::Done)
            }
        })
    };
    m.update("a", Spec::new(slow.clone()));
    sleep(Duration::from_secs(1)).await; // first run in progress
    for _ in 0..10 {
        m.update("a", Spec::new(slow.clone()));
    }
    sleep(Duration::from_secs(60)).await;
    assert_eq!(n.load(SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn stop_wins_over_a_simultaneous_update() {
    let m = manager();
    let n = Arc::new(AtomicUsize::new(0));
    m.update("a", Spec::new(counting(&n, Flow::Done)));
    sleep(Duration::from_millis(1)).await;
    assert_eq!(n.load(SeqCst), 1);

    m.update("a", Spec::new(counting(&n, Flow::Done)));
    m.remove("a").unwrap().await;
    assert_eq!(n.load(SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn remove_cancels_the_job_and_runs_on_stop_once() {
    let m = manager();
    let stopped = Arc::new(AtomicUsize::new(0));
    let on_stop = {
        let stopped = stopped.clone();
        stop_fn(move || {
            let stopped = stopped.clone();
            async move {
                stopped.fetch_add(1, SeqCst);
                Ok(())
            }
        })
    };
    let wait_for_cancel = job(|token| async move {
        token.cancelled().await;
        Ok(Flow::Done)
    });
    m.update("a", Spec::new(wait_for_cancel).on_stop(on_stop));
    sleep(Duration::from_millis(1)).await;

    let termination = m.remove("a").expect("controller exists");
    timeout(Duration::from_secs(1), termination)
        .await
        .expect("terminates");
    assert_eq!(stopped.load(SeqCst), 1);
    assert!(m.remove("a").is_none());
    timeout(Duration::from_secs(1), m.termination("a"))
        .await
        .expect("unknown name resolves at once");
}

#[tokio::test(start_paused = true)]
async fn on_stop_is_bounded_by_its_timeout() {
    let m = manager();
    let hang = stop_fn(|| async {
        std::future::pending::<()>().await;
        Ok(())
    });
    m.update(
        "a",
        Spec::new(job(|_| async { Ok(Flow::Done) }))
            .on_stop(hang)
            .stop_timeout(Duration::from_secs(2)),
    );
    sleep(Duration::from_millis(1)).await;
    timeout(Duration::from_secs(5), m.remove("a").unwrap())
        .await
        .expect("stop is bounded");
}

#[tokio::test(start_paused = true)]
async fn status_tracks_failures_and_recovery() {
    let registry = Registry::new();
    let m = Manager::builder().registry(registry.clone()).build();
    let calls = Arc::new(AtomicUsize::new(0));
    let flaky = {
        let calls = calls.clone();
        job(move |_| {
            let calls = calls.clone();
            async move {
                if calls.fetch_add(1, SeqCst) < 2 {
                    Err("not yet".into())
                } else {
                    Ok(Flow::Done)
                }
            }
        })
    };
    m.update("flaky", Spec::new(flaky));
    sleep(Duration::from_millis(1500)).await; // failed at t = 0 and 1
    let s = &m.statuses()[0];
    assert_eq!((s.failure_count, s.consecutive_failures), (2, 2));
    assert_eq!(s.last_error.as_deref(), Some("not yet"));

    sleep(Duration::from_secs(5)).await; // succeeded at t = 3
    let s = &registry.statuses()[0];
    assert_eq!(s.name, "flaky");
    assert_eq!(
        (s.success_count, s.failure_count, s.consecutive_failures),
        (1, 2, 0)
    );
    assert_eq!(s.last_error, None);
    assert!(
        serde_json::to_string(s)
            .unwrap()
            .contains("\"name\":\"flaky\"")
    );

    m.remove("flaky").unwrap().await;
    assert!(registry.statuses().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_panicking_job_is_recorded_and_retried() {
    let m = manager();
    let calls = Arc::new(AtomicUsize::new(0));
    let panics_once = {
        let calls = calls.clone();
        job(move |_| {
            let calls = calls.clone();
            async move {
                if calls.fetch_add(1, SeqCst) == 0 {
                    panic!("kaboom");
                }
                Ok(Flow::Done)
            }
        })
    };
    m.update("a", Spec::new(panics_once));
    sleep(Duration::from_millis(1)).await;
    let s = &m.statuses()[0];
    assert_eq!(s.failure_count, 1);
    assert!(s.last_error.as_deref().unwrap().contains("kaboom"));

    sleep(Duration::from_secs(2)).await;
    assert_eq!(m.statuses()[0].success_count, 1);
}

#[tokio::test(start_paused = true)]
async fn max_interval_caps_the_interval() {
    let m = Manager::builder()
        .registry(Registry::new())
        .max_interval(Duration::from_secs(5))
        .build();
    let n = Arc::new(AtomicUsize::new(0));
    m.update(
        "a",
        Spec::new(counting(&n, Flow::Continue)).interval(Duration::from_secs(60)),
    );
    sleep(Duration::from_secs(11)).await;
    assert_eq!(n.load(SeqCst), 3); // t = 0, 5, 10
    assert_eq!(m.statuses()[0].interval, Some(Duration::from_secs(5)));
}

#[tokio::test(start_paused = true)]
async fn dropping_the_manager_stops_its_controllers() {
    let m = manager();
    let stopped = Arc::new(AtomicUsize::new(0));
    let on_stop = {
        let stopped = stopped.clone();
        stop_fn(move || {
            let stopped = stopped.clone();
            async move {
                stopped.fetch_add(1, SeqCst);
                Ok(())
            }
        })
    };
    m.update(
        "a",
        Spec::new(job(|token| async move {
            token.cancelled().await;
            Ok(Flow::Done)
        }))
        .on_stop(on_stop),
    );
    sleep(Duration::from_millis(1)).await;

    let termination = m.termination("a");
    drop(m);
    timeout(Duration::from_secs(1), termination)
        .await
        .expect("terminates");
    assert_eq!(stopped.load(SeqCst), 1);
}
