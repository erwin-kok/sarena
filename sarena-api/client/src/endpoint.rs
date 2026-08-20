use std::sync::Arc;

use sarena_api_types_v1::endpoint;

use crate::{api_client::ApiClientInner, error::Res, transport::Transport};

pub struct EndpointClient<T: Transport + 'static> {
    inner: Arc<ApiClientInner<T>>,
}

impl<T: Transport + 'static> EndpointClient<T> {
    pub fn new(inner: Arc<ApiClientInner<T>>) -> Arc<Self> {
        Arc::new(Self { inner })
    }
}

impl<T: Transport + 'static> EndpointClient<T> {
    pub async fn create(
        &self,
        attachment_id: &str,
        ep: &endpoint::EndpointCreateRequest,
    ) -> Res<endpoint::EndpointCreateResponse> {
        self.inner
            .put_api_data(&endpoint_path(attachment_id), ep)
            .await
    }

    pub async fn delete(&self, attachment_id: &str) -> Res<()> {
        self.inner
            .delete_api_data_no_body(&endpoint_path(attachment_id))
            .await
    }

    pub async fn health(&self, attachment_id: &str) -> Res<endpoint::EndpointHealthResponse> {
        self.inner
            .get_api_data(&format!("{}/health", endpoint_path(attachment_id)))
            .await
    }
}

fn endpoint_path(attachment_id: &str) -> String {
    format!("/endpoint/{attachment_id}")
}

pub fn attachment_id(container_id: &str, if_name: &str) -> String {
    format!("cni-attachment-id:{container_id}:{if_name}")
}
