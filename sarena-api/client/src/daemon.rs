use std::sync::Arc;

use http::Method;
use sarena_api_types_v1::daemon;

use crate::{api_client::ApiClientInner, error::Res, transport::Transport};

pub struct DaemonClient<T: Transport + 'static> {
    inner: Arc<ApiClientInner<T>>,
}

impl<T: Transport + 'static> DaemonClient<T> {
    pub fn new(inner: Arc<ApiClientInner<T>>) -> Arc<Self> {
        Arc::new(Self { inner })
    }
}

impl<T: Transport + 'static> DaemonClient<T> {
    pub async fn get_config(&self) -> Res<daemon::DaemonConfigurationResponse> {
        self.inner.get_api_data("/daemon/config").await
    }

    pub async fn health(&self) -> Res<()> {
        self.inner
            .send_and_check(Method::GET, "/daemon/health", None)
            .await?;
        Ok(())
    }
}
