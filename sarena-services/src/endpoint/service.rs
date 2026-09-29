use std::net::IpAddr;

use async_trait::async_trait;
use sarena_api_types_v1::endpoint::{
    EndpointCreateRequest, EndpointCreateResponse, EndpointHealthResponse, EndpointHealthStatus,
};
use sarena_data_plane::{InterfaceAddress, LoaderHandle, MacAddress, add_container};
use tracing::info;

use crate::endpoint::{EndpointService, Res};

pub struct DefaultEndpointService {
    loader_handle: LoaderHandle,
}

impl DefaultEndpointService {
    pub fn new(loader_handle: LoaderHandle) -> Self {
        Self { loader_handle }
    }
}

#[async_trait]
impl EndpointService for DefaultEndpointService {
    async fn create(
        &self,
        attachment_id: String,
        request: EndpointCreateRequest,
    ) -> Res<EndpointCreateResponse> {
        info!("create endpoint {attachment_id}: {:?}", request);

        if let Some(ipv4) = request.ipv4 {
            let container_ip: IpAddr = ipv4.ip.parse::<InterfaceAddress>().expect("parse ip").ip;
            let host_mac = MacAddress::parse(&request.host_mac).expect("parse host mac");
            let container_mac =
                MacAddress::parse(&request.container_mac).expect("parse container mac");

            add_container(
                self.loader_handle.clone(),
                &request.host_iface_name,
                host_mac,
                container_ip,
                container_mac,
            )
            .await;
        }

        Ok(EndpointCreateResponse {})
    }

    async fn delete(&self, attachment_id: String) -> Res<()> {
        info!("delete endpoint {attachment_id}");
        Ok(())
    }

    async fn health(&self, attachment_id: String) -> Res<EndpointHealthResponse> {
        info!("endpoint health {attachment_id}");

        Ok(EndpointHealthResponse {
            heatlh: EndpointHealthStatus::Ok,
        })
    }
}
