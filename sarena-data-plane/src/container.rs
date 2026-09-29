use std::net::{IpAddr, Ipv4Addr};

use sarena_infra::{
    Link as _, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner as _,
    netlink_link::NetlinkLink,
};
use sarena_shared::{EndpointConfig, EndpointInfo};

use crate::{EndpointConfigMap, EndpointKind, LoaderHandle, LxcMap, PIN_ROOT, PinRoot};

pub async fn add_container(
    loader_handle: LoaderHandle,
    iface_name: &str,
    host_mac: MacAddress,
    container_ip: IpAddr,
    container_mac: MacAddress,
) {
    let provisioner = NetlinkNetworkProvisioner;

    let container_ip = match container_ip {
        IpAddr::V4(ipv4_addr) => ipv4_addr,
        IpAddr::V6(_) => panic!("IPv6 peer addresses are not supported"),
    };

    loader_handle
        .add_endpoint(EndpointKind::Container, iface_name)
        .await
        .expect("add endpoint");
    let host = provisioner.get_link(iface_name).await.expect("get_link");
    set_endpoint_config(&host, host_mac, container_ip);
    insert_endpoint_info(&host, host_mac, container_mac, container_ip);
}

fn set_endpoint_config(link: &NetlinkLink, host_mac: MacAddress, container_ip: Ipv4Addr) {
    EndpointConfigMap::for_link(&PinRoot::new(PIN_ROOT), link.ifname())
        .expect("open endpoint_config map")
        .set(EndpointConfig {
            mac: host_mac.0,
            ipv4: container_ip,
        })
        .expect("set endpoint config");
}

fn insert_endpoint_info(
    link: &NetlinkLink,
    host_mac: MacAddress,
    container_mac: MacAddress,
    container_ip: Ipv4Addr,
) {
    LxcMap::open(&PinRoot::new(PIN_ROOT))
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
