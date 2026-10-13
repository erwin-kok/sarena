use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::FutureExt;
use futures::future::{BoxFuture, join_all};
use tokio_util::sync::CancellationToken;

/// A future that resolves once one or more background tasks have fully
/// terminated, including any stop hook they run.
///
/// Dropping it does not affect the tasks; awaiting it is optional.
pub struct Termination(BoxFuture<'static, ()>);

impl Termination {
    pub(crate) fn from_token(done: CancellationToken) -> Self {
        Self(Box::pin(async move { done.cancelled().await }))
    }

    pub(crate) fn ready() -> Self {
        Self(Box::pin(async {}))
    }

    pub(crate) fn all(items: Vec<Termination>) -> Self {
        Self(Box::pin(async move {
            join_all(items).await;
        }))
    }
}

impl Future for Termination {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.0.poll_unpin(cx)
    }
}

impl fmt::Debug for Termination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Termination")
    }
}
