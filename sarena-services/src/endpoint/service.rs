use std::net::IpAddr;

use async_trait::async_trait;
use sarena_api_types_v1::endpoint::{
    EndpointCreateRequest, EndpointCreateResponse, EndpointHealthResponse, EndpointHealthStatus,
};
use sarena_data_plane::{EndpointManager, InterfaceAddress, MacAddress};
use tracing::info;

use crate::endpoint::{EndpointService, Res};

pub struct DefaultEndpointService {
    endpoint_manager: EndpointManager,
}

impl DefaultEndpointService {
    pub fn new(endpoint_manager: EndpointManager) -> Self {
        Self { endpoint_manager }
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

            self.endpoint_manager
                .add_container(
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
