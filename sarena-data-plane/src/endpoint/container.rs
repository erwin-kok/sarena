use sarena_infra::{
    Link as _, NetlinkNetworkProvisioner, NetworkProvisioner as _, netlink_link::NetlinkLink,
};
use sarena_shared::{EndpointConfig, EndpointInfo};

use crate::{
    EndpointConfigMap, EndpointId, EndpointManager, EndpointPersist, EndpointSpec,
    LoaderEndpointKind, LxcMap, PIN_ROOT, PinRoot, Res,
};

impl EndpointManager {
    pub async fn create_endpoint(&self, spec: EndpointSpec) -> Res<EndpointId> {
        let lease = self.allocate_new_id()?;
        let endpoint = EndpointPersist {
            id: lease.id(),
            spec,
        };

        let ep = self.add_endpoint(&endpoint)?;
        ep.regenerate().await?;

        let provisioner = NetlinkNetworkProvisioner;
        self.loader
            .add_loader_endpoint(LoaderEndpointKind::Container, &endpoint.host_iface_name)
            .await?;
        let host = provisioner.get_link(&endpoint.host_iface_name).await?;
        set_endpoint_config(&host, &endpoint)?;
        insert_endpoint_info(&host, &endpoint)?;

        Ok(lease.commit())
    }
}

fn set_endpoint_config(link: &NetlinkLink, endpoint: &EndpointPersist) -> Res<()> {
    EndpointConfigMap::for_link(&PinRoot::new(PIN_ROOT), link.ifname())?.set(EndpointConfig {
        endpoint_id: endpoint.id.get(),
        host_mac: endpoint.host_mac.0,
        ipv4: endpoint.ipv4,
        ipv6: endpoint.ipv6,
    })
}

fn insert_endpoint_info(link: &NetlinkLink, endpoint: &EndpointPersist) -> Res<()> {
    LxcMap::open(&PinRoot::new(PIN_ROOT))?.upsert_endpoint(
        endpoint,
        &EndpointInfo {
            endpoint_id: endpoint.id.get(),
            if_index: link.ifindex(),
            container_mac: endpoint.container_mac.0,
            host_mac: endpoint.host_mac.0,
        },
    )
}
