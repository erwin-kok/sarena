use async_trait::async_trait;
use sarena_api_types_v1::daemon::{DaemonConfigurationResponse, DaemonDebugInfoResponse};
use thiserror::Error;

mod service;

pub use service::DefaultDaemonService;

#[derive(Debug, Error)]
pub enum DaemonError {}

pub type Res<T> = Result<T, DaemonError>;

#[async_trait]
pub trait DaemonService: Send + Sync {
    async fn config(&self) -> Res<DaemonConfigurationResponse>;
    async fn health(&self) -> Res<()>;
    async fn debuginfo(&self) -> Res<DaemonDebugInfoResponse>;
}
