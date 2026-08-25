use std::net::{IpAddr, Ipv4Addr};

use async_trait::async_trait;
use aya::maps::{Array, Map, MapData, hash_map};
use sarena_api_types_v1::endpoint::{
    EndpointCreateRequest, EndpointCreateResponse, EndpointHealthResponse, EndpointHealthStatus,
};
use sarena_infra::{
    InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner as _,
    netlink_link::NetlinkLink,
};
use sarena_loader::{EndpointHandle, EndpointKind, LoaderHandle, PinRoot};
use sarena_shared::{EndpointConfig, EndpointInfo, Ipv4Key, Ipv4KeyExt as _};
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

        let host = self
            .netlink_provisioner
            .get_link(&request.host_iface_name)
            .await
            .expect("get_link");

        let handle = self
            .loader_handle
            .add_endpoint(EndpointKind::Container, &request.host_iface_name)
            .await
            .expect("add endpoint");

        if let Some(ipv4) = request.ipv4 {
            let peer_ip: IpAddr = ipv4.ip.parse::<InterfaceAddress>().expect("parse ip").ip;
            let peer_ip = match peer_ip {
                IpAddr::V4(ipv4_addr) => ipv4_addr,
                IpAddr::V6(_) => panic!("IPv6 peer addresses are not supported"),
            };

            let host_mac = MacAddress::parse(&request.host_mac).expect("parse host mac");
            Self::set_endpoint_config(&handle, host_mac, peer_ip);

            let peer_mac = MacAddress::parse(&request.container_mac).expect("parse container mac");
            self.insert_endpoint_info(peer_ip, &host, peer_mac);
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
    fn set_endpoint_config(handle: &EndpointHandle, host_mac: MacAddress, peer_ip: Ipv4Addr) {
        let path = &handle.map_paths["endpoint_config"];
        let map_data = MapData::from_pin(path).expect("map from_pin");
        let map = Map::Array(map_data);
        let mut array: Array<_, EndpointConfig> = Array::try_from(map).expect("try_from");
        array
            .set(
                0,
                EndpointConfig {
                    mac: host_mac.0,
                    ipv4: peer_ip,
                },
                0,
            )
            .expect("setting element");
    }

    fn insert_endpoint_info(&self, peer_ip: Ipv4Addr, link: &NetlinkLink, peer_mac: MacAddress) {
        let pin_root = PinRoot::new(&self.pin_root);
        let path = pin_root.global_map_dir("lxc_map");
        let map_data = MapData::from_pin(path).expect("map from_pin");
        let map = Map::from_map_data(map_data).expect("from_map_data");

        let mut lxc_map: hash_map::HashMap<_, Ipv4Key, EndpointInfo> =
            hash_map::HashMap::try_from(map).expect("try_from");

        let key = Ipv4Key::from_addr(peer_ip);
        let value = EndpointInfo {
            if_index: link.ifindex(),
            mac: peer_mac.0,
        };
        lxc_map.insert(key, value, 0).expect("insert element");
    }
}
