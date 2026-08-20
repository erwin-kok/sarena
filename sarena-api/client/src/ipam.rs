use std::sync::Arc;

use sarena_api_types_v1::ipam;

use crate::{api_client::ApiClientInner, error::Res, transport::Transport};

pub struct IpamClient<T: Transport + 'static> {
    inner: Arc<ApiClientInner<T>>,
}

impl<T: Transport + 'static> IpamClient<T> {
    pub fn new(inner: Arc<ApiClientInner<T>>) -> Arc<Self> {
        Arc::new(Self { inner })
    }
}

impl<T: Transport + 'static> IpamClient<T> {
    pub async fn allocate(
        &self,
        owner: String,
        pool: Option<String>,
    ) -> Res<ipam::IpamAllocateResponse> {
        self.inner
            .put_api_data("/ipam", &ipam::IpamAllocateRequest { owner, pool })
            .await
    }

    pub async fn release(&self, ip: String, pool: Option<String>) -> Res<()> {
        self.inner
            .delete_api_data("/ipam", &ipam::IpamReleaseRequest { ip, pool })
            .await
    }
}
