use crate::{
    Link, Netns, NetworkProvisioner, Res, VethPair, VethSpec,
    netlink_link::{self, NetlinkLink, create_veth_pair},
};

#[derive(Debug, Default, Clone, Copy)]
pub struct NetlinkNetworkProvisioner;

impl NetworkProvisioner for NetlinkNetworkProvisioner {
    type LinkType = NetlinkLink;

    async fn create_netns(&mut self, name: &str) -> Res<()> {
        Netns::create(name).await
    }

    async fn delete_netns(&mut self, name: &str) -> Res<()> {
        Netns::delete(name)
    }

    async fn create_veth(&mut self, spec: VethSpec) -> Res<VethPair<Self::LinkType>> {
        let (mut host, mut peer) = create_veth_pair(&spec.host_ifname, &spec.peer_ifname).await?;

        if let Some(mac) = spec.host_mac {
            host.set_mac(mac).await?;
        }
        if let Some(mac) = spec.peer_mac {
            peer.set_mac(mac).await?;
        }

        // Re-fetch both ends: a MAC change invalidates the `Link`
        // snapshots above (`.mac` is now stale), and the final MACs are
        // needed either way -- the ones just set, or kernel-random ones if
        // the caller didn't ask for specific values.
        let host = self.get_link(&spec.host_ifname).await?;
        let mut peer = self.get_link(&spec.peer_ifname).await?;

        // Move the peer *after* reading its state back -- we already have
        // what we need from the default-namespace fetch above, and
        // `set_ns` itself only needs the ifindex, which doesn't change.
        peer.set_ns(&spec.peer_netns).await?;

        Ok(VethPair { host, peer })
    }

    async fn delete_veth(&mut self, veth_pair: &mut VethPair<Self::LinkType>) -> Res<()> {
        // deleting the host will automatically also delete the peer.
        veth_pair.host.delete().await
    }

    async fn get_link(&self, name: &str) -> Res<Self::LinkType> {
        netlink_link::get_link_by_name(name).await
    }

    async fn get_link_in_ns(&self, ns: &str, name: &str) -> Res<Self::LinkType> {
        netlink_link::get_link_by_name_in_ns(ns, name).await
    }

    async fn list_links(&self) -> Res<Vec<Self::LinkType>> {
        netlink_link::list_links().await
    }

    async fn list_links_in_ns(&self, ns: &str) -> Res<Vec<Self::LinkType>> {
        netlink_link::list_links_in_ns(ns).await
    }
}
