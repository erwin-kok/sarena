use async_trait::async_trait;
use sarena_api_types_v1::endpoint::{
    EndpointCreateRequest, EndpointCreateResponse, EndpointHealthResponse, EndpointHealthStatus,
};

use crate::{EndpointError, EndpointService};

#[derive(Debug, Default, Clone, Copy)]
pub struct MockEndpointService;

#[async_trait]
impl EndpointService for MockEndpointService {
    async fn create(
        &self,
        _attachment_id: String,
        _request: EndpointCreateRequest,
    ) -> Result<EndpointCreateResponse, EndpointError> {
        Ok(EndpointCreateResponse {})
    }

    async fn delete(&self, _attachment_id: String) -> Result<(), EndpointError> {
        Ok(())
    }

    async fn health(
        &self,
        _attachment_id: String,
    ) -> Result<EndpointHealthResponse, EndpointError> {
        Ok(EndpointHealthResponse {
            heatlh: EndpointHealthStatus::Ok,
        })
    }
}
