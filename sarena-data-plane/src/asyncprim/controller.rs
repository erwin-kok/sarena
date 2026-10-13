use std::{
    fmt,
    future::Future,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use futures::{FutureExt, future::BoxFuture};
use serde::Serialize;
use tokio::{sync::watch, time::Instant};
use tokio_util::sync::CancellationToken;

use crate::asyncprim::util::{lock, panic_message};

pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// The work a controller runs. It receives a token that is cancelled when the
/// controller is removed.
pub type JobFn =
    Arc<dyn Fn(CancellationToken) -> BoxFuture<'static, Result<Flow, BoxError>> + Send + Sync>;

/// Cleanup that runs once when a controller stops.
pub type StopFn = Arc<dyn Fn() -> BoxFuture<'static, Result<(), BoxError>> + Send + Sync>;

/// Wraps an async closure as a [`JobFn`].
pub fn job<F, Fut>(f: F) -> JobFn
where
    F: Fn(CancellationToken) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Flow, BoxError>> + Send + 'static,
{
    Arc::new(move |token| Box::pin(f(token)))
}

/// Wraps an async closure as a [`StopFn`].
pub fn stop_fn<F, Fut>(f: F) -> StopFn
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), BoxError>> + Send + 'static,
{
    Arc::new(move || Box::pin(f()))
}

/// What a successful run asks the controller to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Run again after the spec's interval. With no interval, wait for the
    /// next update.
    Continue,
    /// Wait for the next update, even when an interval is set.
    Done,
}

/// How long to wait before retrying a failed run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Backoff {
    /// No backoff: after an error, wait for the regular interval, or for the
    /// next update when there is no interval.
    None,
    /// `base × consecutive errors`, capped at `max`.
    Linear { base: Duration, max: Duration },
    /// `base × 2^(consecutive errors − 1)`, capped at `max`.
    Exponential { base: Duration, max: Duration },
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff::Exponential {
            base: Duration::from_secs(1),
            max: Duration::from_secs(300),
        }
    }
}

impl Backoff {
    /// The delay before the next attempt after `consecutive_errors` failures.
    pub fn delay(&self, consecutive_errors: u32) -> Option<Duration> {
        let n = consecutive_errors.max(1);
        match *self {
            Backoff::None => None,
            Backoff::Linear { base, max } => Some(base.saturating_mul(n).min(max)),
            Backoff::Exponential { base, max } => {
                let mut delay = base.min(max);

                if delay.is_zero() {
                    return Some(delay);
                }

                for _ in 1..n {
                    if delay >= max {
                        break;
                    }

                    delay = delay.saturating_mul(2).min(max);
                }

                Some(delay)
            }
        }
    }
}

/// The configuration of a controller. Passing a new `Spec` for an existing
/// name replaces the old one and runs the job again at once.
#[derive(Clone)]
pub struct Spec {
    pub job: JobFn,
    pub on_stop: Option<StopFn>,
    /// `None` runs the job until it succeeds once, then waits for an update.
    pub interval: Option<Duration>,
    pub backoff: Backoff,
    /// The longest `on_stop` may run before it is abandoned.
    pub stop_timeout: Duration,
}

impl Spec {
    pub fn new(job: JobFn) -> Self {
        Self {
            job,
            on_stop: None,
            interval: None,
            backoff: Backoff::default(),
            stop_timeout: Duration::from_secs(30),
        }
    }

    #[must_use]
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = Some(interval);
        self
    }

    #[must_use]
    pub fn backoff(mut self, backoff: Backoff) -> Self {
        self.backoff = backoff;
        self
    }

    #[must_use]
    pub fn on_stop(mut self, on_stop: StopFn) -> Self {
        self.on_stop = Some(on_stop);
        self
    }

    #[must_use]
    pub fn stop_timeout(mut self, timeout: Duration) -> Self {
        self.stop_timeout = timeout;
        self
    }
}

impl fmt::Debug for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Spec")
            .field("interval", &self.interval)
            .field("backoff", &self.backoff)
            .field("has_on_stop", &self.on_stop.is_some())
            .field("stop_timeout", &self.stop_timeout)
            .finish_non_exhaustive()
    }
}

/// Receives a callback after every controller run, for metrics.
pub trait ControllerObserver: Send + Sync {
    fn run_finished(&self, _name: &str, _success: bool, _duration: Duration) {}
}

/// A point-in-time view of a controller's state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ControllerStatus {
    pub name: String,
    pub id: u64,
    pub interval: Option<Duration>,
    pub backoff: Backoff,
    pub success_count: u64,
    pub failure_count: u64,
    pub consecutive_failures: u64,
    pub last_success: Option<SystemTime>,
    pub last_failure: Option<SystemTime>,
    pub last_error: Option<String>,
    pub last_duration: Option<Duration>,
}

pub(crate) struct StatusCell(Mutex<ControllerStatus>);

impl StatusCell {
    pub(crate) fn new(name: &str, id: u64, spec: &Spec) -> Self {
        Self(Mutex::new(ControllerStatus {
            name: name.to_string(),
            id,
            interval: spec.interval,
            backoff: spec.backoff,
            success_count: 0,
            failure_count: 0,
            consecutive_failures: 0,
            last_success: None,
            last_failure: None,
            last_error: None,
            last_duration: None,
        }))
    }

    pub(crate) fn configure(&self, spec: &Spec) {
        let mut s = lock(&self.0);
        s.interval = spec.interval;
        s.backoff = spec.backoff;
    }

    fn record_run(&self, result: &Result<Flow, BoxError>, duration: Duration) -> bool {
        let mut s = lock(&self.0);
        s.last_duration = Some(duration);
        match result {
            Ok(_) => {
                s.success_count += 1;
                s.consecutive_failures = 0;
                s.last_success = Some(SystemTime::now());
                s.last_error = None;
                true
            }
            Err(e) => {
                s.failure_count += 1;
                s.consecutive_failures += 1;
                s.last_failure = Some(SystemTime::now());
                s.last_error = Some(e.to_string());
                false
            }
        }
    }

    fn record_stop_error(&self, message: String) {
        let mut s = lock(&self.0);
        s.failure_count += 1;
        s.last_failure = Some(SystemTime::now());
        s.last_error = Some(message);
    }

    pub(crate) fn snapshot(&self) -> ControllerStatus {
        lock(&self.0).clone()
    }
}

/// The state owned by one controller task.
pub(crate) struct Runner {
    pub(crate) name: String,
    pub(crate) spec_rx: watch::Receiver<Spec>,
    pub(crate) stop: CancellationToken,
    pub(crate) status: Arc<StatusCell>,
    pub(crate) observer: Option<Arc<dyn ControllerObserver>>,
}

impl Runner {
    pub(crate) async fn run(mut self) {
        let mut spec = self.spec_rx.borrow_and_update().clone();
        let mut errors: u32 = 0;

        loop {
            let started = Instant::now();
            let job = spec.job.clone();
            let token = self.stop.clone();
            // The call happens inside the async block, so a panic while
            // building the future is caught as well.
            let result = match AssertUnwindSafe(async move { job(token).await })
                .catch_unwind()
                .await
            {
                Ok(result) => result,
                Err(payload) => Err(panic_message(payload).into()),
            };
            let elapsed = started.elapsed();
            let success = self.status.record_run(&result, elapsed);
            if let Some(observer) = &self.observer {
                observer.run_finished(&self.name, success, elapsed);
            }

            let wait = match &result {
                Ok(Flow::Continue) => {
                    errors = 0;
                    spec.interval
                }
                Ok(Flow::Done) => {
                    errors = 0;
                    None
                }
                Err(e) => {
                    errors = errors.saturating_add(1);

                    tracing::debug!(
                        controller = %self.name,
                        error = %e,
                        consecutive = errors,
                        "controller run failed"
                    );

                    spec.backoff.delay(errors).or(spec.interval)
                }
            };

            tokio::select! {
                biased;
                () = self.stop.cancelled() => break,
                changed = self.spec_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    spec = self.spec_rx.borrow_and_update().clone();
                }
                () = sleep_opt(wait) => {}
            }
        }

        tracing::debug!(controller = %self.name, "controller stopping");
        if let Some(on_stop) = spec.on_stop.clone() {
            let stop = AssertUnwindSafe(async move { on_stop().await }).catch_unwind();
            match tokio::time::timeout(spec.stop_timeout, stop).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(e))) => self.status.record_stop_error(e.to_string()),
                Ok(Err(payload)) => self.status.record_stop_error(panic_message(payload)),
                Err(_) => self
                    .status
                    .record_stop_error(format!("stop timed out after {:?}", spec.stop_timeout)),
            }
        }
    }
}

async fn sleep_opt(wait: Option<Duration>) {
    match wait {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending().await,
    }
}
