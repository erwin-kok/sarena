use std::{fmt, time::Duration};

use crate::controller::{
    backoff::Backoff,
    job::{JobFn, StopFn},
};

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
