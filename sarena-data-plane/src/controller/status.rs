use std::{
    sync::Mutex,
    time::{Duration, SystemTime},
};

use serde::Serialize;

use crate::controller::{backoff::Backoff, guard::RunError, job::Flow, spec::Spec};

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

/// The shared, lockable status of one controller. The runner writes it; the
/// manager and registry read snapshots.
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
        let mut s = self.lock();
        s.interval = spec.interval;
        s.backoff = spec.backoff;
    }

    pub(crate) fn record_run(&self, result: &Result<Flow, RunError>, duration: Duration) {
        let mut s = self.lock();
        s.last_duration = Some(duration);
        match result {
            Ok(_) => {
                s.success_count += 1;
                s.consecutive_failures = 0;
                s.last_success = Some(SystemTime::now());
                s.last_error = None;
            }
            Err(e) => {
                s.failure_count += 1;
                s.consecutive_failures += 1;
                s.last_failure = Some(SystemTime::now());
                s.last_error = Some(e.to_string());
            }
        }
    }

    pub(crate) fn record_stop_error(&self, error: &RunError) {
        let message = match error {
            RunError::TimedOut(limit) => format!("stop timed out after {limit:?}"),
            RunError::Failed(_) | RunError::Panicked(_) => error.to_string(),
        };

        let mut s = self.lock();
        s.failure_count += 1;
        s.last_failure = Some(SystemTime::now());
        s.last_error = Some(message);
    }

    pub(crate) fn snapshot(&self) -> ControllerStatus {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ControllerStatus> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
