use std::sync::Arc;

use sarena_api_types_v1::{EndpointCreateRequest, EndpointCreateResponse};

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
    pub async fn create(&self, ep: &EndpointCreateRequest) -> Res<EndpointCreateResponse> {
        self.inner.put_api_data("/endpoint", ep).await
    }
}
