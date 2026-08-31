use sarena_infra::{
    InfraError, InterfaceAddress, Link, MacAddress, NetlinkNetworkProvisioner,
    NetworkProvisioner as _, VethSpec, netlink_link::NetlinkLink,
};

use crate::{ControlPlaneError, Res};

pub const SARENA_HOST: &str = "sarena_host";
pub const SARENA_NET: &str = "sarena_net";

pub async fn setup_host_device(
    provisioner: &mut NetlinkNetworkProvisioner,
    device_mtu: u32,
    addr: InterfaceAddress,
) -> Res<(NetlinkLink, NetlinkLink)> {
    let link = provisioner.get_link(SARENA_HOST).await;
    if matches!(&link, Err(InfraError::LinkNotFound(_))) {
        let host_mac = MacAddress::generate_rand();
        let peer_mac = MacAddress::generate_rand();
        let _ = provisioner
            .create_veth(VethSpec {
                host_ifname: SARENA_HOST.to_string(),
                peer_ifname: SARENA_NET.to_string(),
                host_mac: Some(host_mac),
                peer_mac: Some(peer_mac),
            })
            .await
            .map_err(|e| ControlPlaneError::CouldNotCreateVethPair(e.to_string()))?;
    } else {
        link.map_err(|src| ControlPlaneError::LinkLookup {
            name: SARENA_HOST.to_string(),
            src: src.to_string(),
        })?;
    }
    let mut host = setup_link(provisioner, SARENA_HOST, device_mtu).await?;
    let net = setup_link(provisioner, SARENA_NET, device_mtu).await?;

    host.replace_addr(addr).await?;

    Ok((host, net))
}

async fn setup_link(
    provisioner: &mut NetlinkNetworkProvisioner,
    name: &str,
    device_mtu: u32,
) -> Res<NetlinkLink> {
    let mut link =
        provisioner
            .get_link(name)
            .await
            .map_err(|src| ControlPlaneError::LinkLookup {
                name: SARENA_HOST.to_string(),
                src: src.to_string(),
            })?;

    link.set_up().await?;

    link.set_ipv4_forwarding(true).await?;
    link.set_rp_filter(0).await?;
    link.set_accept_local(true).await?;
    link.set_send_redirects(false).await?;

    link.set_arp(false).await?;
    link.set_mtu(device_mtu).await?;

    Ok(link)
}
