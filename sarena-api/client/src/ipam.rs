use std::{collections::HashMap, sync::Arc};

use sarena_api_types_v1::ipam;

use crate::{PathBuilder, api_client::ApiClientInner, error::Res, transport::Transport};

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
        family: Option<String>,
        owner: Option<String>,
        pool: Option<String>,
        expiration: bool,
    ) -> Res<ipam::IpamAllocateResponse> {
        let endpoint = PathBuilder::new("/ipam")
            .query_opt("family", family.as_deref())
            .query_opt("owner", owner.as_deref())
            .query_opt("pool", pool.as_deref())
            .build();
        let headers = HashMap::from([("expiration".to_string(), expiration.to_string())]);
        self.inner
            .put_api_data_with_headers(&endpoint, Some(headers))
            .await
    }

    pub async fn allocate_ip(
        &self,
        ip: String,
        owner: Option<String>,
        pool: Option<String>,
    ) -> Res<()> {
        let endpoint = PathBuilder::new(format!("/ipam/{ip}"))
            .query_opt("owner", owner.as_deref())
            .query_opt("pool", pool.as_deref())
            .build();
        self.inner.put_api_data_with_headers(&endpoint, None).await
    }

    pub async fn release(&self, ip: String, pool: Option<String>) -> Res<()> {
        let endpoint = PathBuilder::new(format!("/ipam/{ip}"))
            .query_opt("pool", pool.as_deref())
            .build();
        self.inner.delete_api_data_no_body(&endpoint).await
    }
}
