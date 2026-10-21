use std::{future::Future, sync::Arc};

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

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
