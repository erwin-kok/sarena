use std::net::{IpAddr, Ipv4Addr};

use async_trait::async_trait;
use sarena_api_types_v1::endpoint::{
    EndpointCreateRequest, EndpointCreateResponse, EndpointHealthResponse, EndpointHealthStatus,
};
use sarena_infra::{
    InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner as _,
    netlink_link::NetlinkLink,
};
use sarena_loader::{EndpointConfigMap, EndpointKind, LoaderHandle, LxcMap, PinRoot};
use sarena_shared::{EndpointConfig, EndpointInfo};
use tracing::info;

use crate::{EndpointService, Res};

pub struct DefaultEndpointService {
    loader_handle: LoaderHandle,
    netlink_provisioner: NetlinkNetworkProvisioner,
    pin_root: String,
}

impl DefaultEndpointService {
    pub fn new(
        loader_handle: LoaderHandle,
        netlink_provisioner: NetlinkNetworkProvisioner,
        pin_root: impl Into<String>,
    ) -> Self {
        Self {
            loader_handle,
            netlink_provisioner,
            pin_root: pin_root.into(),
        }
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

        self.loader_handle
            .add_endpoint(EndpointKind::Container, &request.host_iface_name)
            .await
            .expect("add endpoint");

        if let Some(ipv4) = request.ipv4 {
            let container_ip: IpAddr = ipv4.ip.parse::<InterfaceAddress>().expect("parse ip").ip;
            let container_ip = match container_ip {
                IpAddr::V4(ipv4_addr) => ipv4_addr,
                IpAddr::V6(_) => panic!("IPv6 peer addresses are not supported"),
            };

            let host = self
                .netlink_provisioner
                .get_link(&request.host_iface_name)
                .await
                .expect("get_link");

            let host_mac = MacAddress::parse(&request.host_mac).expect("parse host mac");
            self.set_endpoint_config(&host, host_mac, container_ip);

            let container_mac =
                MacAddress::parse(&request.container_mac).expect("parse container mac");
            self.insert_endpoint_info(&host, host_mac, container_mac, container_ip);
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

impl DefaultEndpointService {
    fn set_endpoint_config(
        &self,
        link: &NetlinkLink,
        host_mac: MacAddress,
        container_ip: Ipv4Addr,
    ) {
        EndpointConfigMap::for_link(&PinRoot::new(&self.pin_root), link.ifname())
            .expect("open endpoint_config map")
            .set(EndpointConfig {
                mac: host_mac.0,
                ipv4: container_ip,
            })
            .expect("set endpoint config");
    }

    fn insert_endpoint_info(
        &self,
        link: &NetlinkLink,
        host_mac: MacAddress,
        container_mac: MacAddress,
        container_ip: Ipv4Addr,
    ) {
        LxcMap::open(&PinRoot::new(&self.pin_root))
            .expect("open lxc_map")
            .upsert_endpoint(
                container_ip,
                EndpointInfo {
                    if_index: link.ifindex(),
                    container_mac: container_mac.0,
                    host_mac: host_mac.0,
                },
            )
            .expect("insert endpoint info");
    }
}
