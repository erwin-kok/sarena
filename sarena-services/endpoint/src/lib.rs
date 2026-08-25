use async_trait::async_trait;
use sarena_api_types_v1::endpoint::{
    EndpointCreateRequest, EndpointCreateResponse, EndpointHealthResponse,
};
use thiserror::Error;

mod service;
pub use service::DefaultEndpointService;

#[cfg(feature = "test-util")]
mod mock;

#[cfg(feature = "test-util")]
pub use mock::MockEndpointService;

#[derive(Debug, Error)]
pub enum EndpointError {}

pub type Res<T> = Result<T, EndpointError>;

#[async_trait]
pub trait EndpointService: Send + Sync {
    async fn create(
        &self,
        attachment_id: String,
        request: EndpointCreateRequest,
    ) -> Res<EndpointCreateResponse>;

    async fn delete(&self, attachment_id: String) -> Res<()>;

    async fn health(&self, attachment_id: String) -> Res<EndpointHealthResponse>;
}
