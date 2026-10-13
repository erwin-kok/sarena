mod controller;
mod eventqueue;
mod manager;
mod termination;
mod util;

pub use controller::{
    Backoff, BoxError, ControllerObserver, ControllerStatus, Flow, JobFn, Spec, StopFn, job,
    stop_fn,
};
pub use eventqueue::{Cancelled, Event, EventQueue, Pending, TryEnqueueError};
pub use manager::{Manager, ManagerBuilder, Registry};
pub use termination::Termination;

/// Returned when work is submitted to a queue or trigger that has been stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("stopped")]
pub struct Stopped;
