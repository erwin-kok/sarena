use std::net::Ipv4Addr;

use sarena_infra::{
    Link as _, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner as _,
    netlink_link::NetlinkLink,
};
use sarena_shared::{EndpointConfig, EndpointInfo};

use crate::{
    DataPlaneError, EndpointConfigMap, EndpointId, EndpointManager, EndpointPersist, EndpointSpec,
    LoaderEndpointKind, LxcMap, PIN_ROOT, PinRoot, Res,
};

impl EndpointManager {
    pub async fn create_endpoint(&self, spec: EndpointSpec) -> Res<EndpointId> {
        let lease = self.allocate_new_id()?;
        let endpoint = EndpointPersist {
            id: lease.id(),
            spec,
        };
        let provisioner = NetlinkNetworkProvisioner;

        let Some(container_ip) = endpoint.ipv4 else {
            return Err(DataPlaneError::GeneralError(
                "IPv6 peer addresses are not supported".to_string(),
            ));
        };

        self.loader_handle
            .add_loader_endpoint(LoaderEndpointKind::Container, &endpoint.host_iface_name)
            .await?;
        let host = provisioner.get_link(&endpoint.host_iface_name).await?;
        set_endpoint_config(&host, endpoint.host_mac, container_ip)?;
        insert_endpoint_info(
            &host,
            endpoint.host_mac,
            endpoint.container_mac,
            container_ip,
        )?;

        Ok(lease.commit())
    }
}

fn set_endpoint_config(
    link: &NetlinkLink,
    host_mac: MacAddress,
    container_ip: Ipv4Addr,
) -> Res<()> {
    EndpointConfigMap::for_link(&PinRoot::new(PIN_ROOT), link.ifname())?.set(EndpointConfig {
        mac: host_mac.0,
        ipv4: container_ip,
    })
}

fn insert_endpoint_info(
    link: &NetlinkLink,
    host_mac: MacAddress,
    container_mac: MacAddress,
    container_ip: Ipv4Addr,
) -> Res<()> {
    LxcMap::open(&PinRoot::new(PIN_ROOT))?.upsert_endpoint(
        container_ip,
        EndpointInfo {
            if_index: link.ifindex(),
            container_mac: container_mac.0,
            host_mac: host_mac.0,
        },
    )
}
