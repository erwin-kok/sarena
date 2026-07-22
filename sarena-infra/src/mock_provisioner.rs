use crate::{
    InfraError, Link, MacAddress, NetworkProvisioner, Res, VethPair, VethSpec, mock_link::MockLink,
};

#[derive(Debug, Default)]
pub struct MockNetworkProvisioner {
    pub netns_created: Vec<String>,
    pub netns_deleted: Vec<String>,
    pub veths_created: Vec<VethSpec>,
    pub veths_deleted: Vec<VethPair<MockLink>>,
    /// Snapshot registry backing `get_link`/`list_links`. Only updated by
    /// `create_veth`/`delete_veth`/`add_link` -- mutating a `MockLink` a
    /// test is already holding (via `LinkHandle`) does not update this, so
    /// registry queries only reflect provisioner-level operations.
    pub links: Vec<MockLink>,
    next_ifindex: u32,
}

impl NetworkProvisioner for MockNetworkProvisioner {
    type LinkType = MockLink;

    async fn create_netns(&mut self, name: &str) -> Res<()> {
        self.netns_created.push(name.to_owned());
        Ok(())
    }

    async fn delete_netns(&mut self, name: &str) -> Res<()> {
        self.netns_deleted.push(name.to_owned());
        Ok(())
    }

    async fn create_veth(&mut self, spec: VethSpec) -> Res<VethPair<Self::LinkType>> {
        self.next_ifindex += 1;
        let host_ifindex = self.next_ifindex;
        self.next_ifindex += 1;
        let peer_ifindex = self.next_ifindex;

        let pair = VethPair {
            host: MockLink {
                ifname: spec.host_ifname.clone(),
                ifindex: host_ifindex,
                mac: spec.host_mac.unwrap_or(MacAddress([
                    0x02,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    host_ifindex as u8,
                ])),
                peer_ifname: Some(spec.peer_ifname.clone()),
                ..Default::default()
            },
            peer: MockLink {
                ifname: spec.peer_ifname.clone(),
                ifindex: peer_ifindex,
                mac: spec.peer_mac.unwrap_or(MacAddress([
                    0x02,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    peer_ifindex as u8,
                ])),
                peer_ifname: Some(spec.host_ifname.clone()),
                ..Default::default()
            },
        };
        self.links.push(pair.host.clone());
        self.links.push(pair.peer.clone());
        self.veths_created.push(spec);
        Ok(pair)
    }

    async fn delete_veth(&mut self, veth_pair: &mut VethPair<Self::LinkType>) -> Res<()> {
        self.veths_deleted.push(veth_pair.clone());
        veth_pair.host.delete().await?;
        veth_pair.peer.delete().await?;
        Ok(())
    }

    async fn get_link(&self, name: &str) -> Res<Self::LinkType> {
        self.get_link_in_ns_impl(None, name)
    }

    async fn get_link_in_ns(&self, ns: &str, name: &str) -> Res<Self::LinkType> {
        self.get_link_in_ns_impl(Some(ns), name)
    }

    async fn list_links(&self) -> Res<Vec<Self::LinkType>> {
        Ok(self.list_links_in_ns_impl(None))
    }

    async fn list_links_in_ns(&self, ns: &str) -> Res<Vec<Self::LinkType>> {
        Ok(self.list_links_in_ns_impl(Some(ns)))
    }
}

impl MockNetworkProvisioner {
    /// Seed the registry with a link the test didn't create through
    /// `create_veth` -- e.g. `lo` or a physical NIC that's expected to
    /// already exist.
    pub fn add_link(&mut self, link: MockLink) {
        self.links.push(link);
    }

    fn get_link_in_ns_impl(&self, ns: Option<&str>, name: &str) -> Res<MockLink> {
        self.links
            .iter()
            .find(|l| l.ifname == name && l.netns.as_deref() == ns)
            .cloned()
            .ok_or_else(|| InfraError::LinkNotFound(name.to_owned()))
    }

    fn list_links_in_ns_impl(&self, ns: Option<&str>) -> Vec<MockLink> {
        self.links
            .iter()
            .filter(|l| l.netns.as_deref() == ns)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MacAddress, Res};

    async fn provision_and_start_port<P: NetworkProvisioner>(
        provisioner: &mut P,
        peer_netns: &str,
    ) -> Res<VethPair<P::LinkType>> {
        let mut pair = provisioner
            .create_veth(VethSpec {
                host_ifname: "veth-test0".to_owned(),
                peer_ifname: "veth-test1".to_owned(),
                peer_netns: peer_netns.to_owned(),
                host_mac: Some(MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])),
                peer_mac: None,
            })
            .await?;

        // The point of the split: this reads as an operation on the link
        // itself, and works identically whether `pair.host` is a real
        // `Link` or a `MockLink`.
        pair.host.set_up().await?;

        Ok(pair)
    }

    #[tokio::test]
    async fn mock_records_veth_creation_and_link_operations() {
        let mut mock = MockNetworkProvisioner::default();
        mock.create_netns("test-ns").await.unwrap();

        let pair = provision_and_start_port(&mut mock, "test-ns")
            .await
            .unwrap();

        assert_eq!(
            pair.host.mac(),
            MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])
        );
        assert_eq!(pair.host.up_calls, 1);
        assert_eq!(mock.veths_created.len(), 1);
        assert_eq!(mock.netns_created, vec!["test-ns".to_owned()]);
    }

    // The real DataplaneInfraProvisioner would be exercised the same way,
    // generically, in an `#[ignore]`d integration test with root/CAP_*:
    //
    // #[tokio::test]
    // #[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN"]
    // async fn real_provisioner_creates_veth_pair() {
    //     let mut real = DataplaneInfraProvisioner;
    //     let ns = real.create_netns("dpi-prov-test").await.unwrap();
    //     let pair = provision_and_start_port(&mut real, ns.clone()).await.unwrap();
    //     assert_ne!(pair.host.ifindex(), 0);
    //     real.delete_netns(&ns).await.unwrap();
    // }
}
