use std::{any::Any, future::Future, panic::AssertUnwindSafe, time::Duration};

use futures::FutureExt;

use crate::controller::job::BoxError;

/// Why a job or stop hook did not complete successfully.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RunError {
    /// The function returned an error.
    #[error("{0}")]
    Failed(BoxError),
    /// The function panicked; holds the panic message.
    #[error("panicked: {0}")]
    Panicked(String),
    /// The function did not finish within the limit.
    #[error("timed out after {0:?}")]
    TimedOut(Duration),
}

/// Runs `fut`, turning a panic into [`RunError::Panicked`].
///
/// Only a panic while polling `fut` is caught. To also catch one raised while
/// creating the future, build it inside an `async` block.
pub(crate) async fn catch_panic<T>(
    fut: impl Future<Output = Result<T, BoxError>>,
) -> Result<T, RunError> {
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(RunError::Failed(e)),
        Err(payload) => Err(RunError::Panicked(panic_message(payload))),
    }
}

/// Like [`catch_panic`], but fails with [`RunError::TimedOut`] once `limit`
/// has passed.
pub(crate) async fn with_timeout<T>(
    limit: Duration,
    fut: impl Future<Output = Result<T, BoxError>>,
) -> Result<T, RunError> {
    tokio::time::timeout(limit, catch_panic(fut))
        .await
        .unwrap_or(Err(RunError::TimedOut(limit)))
}

fn panic_message(payload: Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
