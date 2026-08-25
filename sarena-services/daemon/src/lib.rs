use async_trait::async_trait;
use sarena_api_types_v1::daemon::DaemonConfigurationResponse;
use thiserror::Error;

mod service;

#[cfg(feature = "test-util")]
mod mock;

#[cfg(feature = "test-util")]
pub use mock::MockDaemonService;
pub use service::DefaultDaemonService;

#[derive(Debug, Error)]
pub enum DaemonError {}

pub type Res<T> = Result<T, DaemonError>;

#[async_trait]
pub trait DaemonService: Send + Sync {
    async fn config(&self) -> Res<DaemonConfigurationResponse>;
    async fn health(&self) -> Res<()>;
}
