use std::{
    net::Ipv4Addr,
    path::{Path, PathBuf},
};

use aya::programs::TcAttachType;
use ipnet::IpNet;

use crate::{InfraError, Link, MacAddress, Netns, PinnedTcxProgram, Res, TcxAttach, route::Route};

#[derive(Debug, Clone, Default)]
pub struct MockProgram {
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct MockLink {
    pub ifname: String,
    pub ifindex: u32,
    pub mac: MacAddress,
    pub peer_ifname: Option<String>,
    pub up_calls: u32,
    pub down_calls: u32,
    pub mtu_calls: Vec<u32>,
    pub mac_calls: Vec<MacAddress>,
    pub addr_calls: Vec<IpNet>,
    pub gateway_calls: Vec<Ipv4Addr>,
    pub route_calls: Vec<Route>,
    pub ipv4_forwarding_calls: Vec<bool>,
    pub ipv6_forwarding_calls: Vec<bool>,
    pub ipv6_disable_calls: Vec<bool>,
    pub rp_filter_calls: Vec<u8>,
    pub rename_calls: Vec<String>,
    pub setns_calls: Vec<PathBuf>,
    pub delete_calls: u32,
    pub netns: Option<PathBuf>,
    pub tcx_upsert_calls: Vec<(String, TcAttachType)>,
    pub tcx_has_link_calls: Vec<(String, TcAttachType)>,
    pub has_tcx_link_result: bool,
    pub next_link_id: u32,
}

#[allow(clippy::unused_async_trait_impl)]
impl Link for MockLink {
    fn ifname(&self) -> &str {
        &self.ifname
    }

    fn ifindex(&self) -> u32 {
        self.ifindex
    }

    fn mac(&self) -> MacAddress {
        self.mac
    }

    async fn set_up(&mut self) -> Res<()> {
        self.up_calls += 1;
        Ok(())
    }

    async fn set_down(&mut self) -> Res<()> {
        self.down_calls += 1;
        Ok(())
    }

    async fn set_mtu(&mut self, mtu: u32) -> Res<()> {
        self.mtu_calls.push(mtu);
        Ok(())
    }

    async fn set_mac(&mut self, mac: MacAddress) -> Res<()> {
        self.mac_calls.push(mac);
        self.mac = mac;
        Ok(())
    }

    async fn set_ns(&mut self, target: &Netns) -> Res<()> {
        self.setns_calls.push(target.path.clone());
        self.netns = Some(target.path.clone());
        Ok(())
    }

    async fn rename(&mut self, new_name: &str) -> Res<()> {
        self.rename_calls.push(new_name.to_owned());
        self.ifname = new_name.to_owned();
        Ok(())
    }

    async fn delete(&mut self) -> Res<()> {
        self.delete_calls += 1;
        Ok(())
    }

    async fn set_addr(&mut self, addr: IpNet) -> Res<()> {
        self.addr_calls.push(addr);
        Ok(())
    }

    async fn add_gateway(&mut self, gateway: Ipv4Addr) -> Res<()> {
        self.gateway_calls.push(gateway);
        Ok(())
    }

    async fn add_route(&mut self, route: &Route) -> Res<()> {
        self.route_calls.push(route.clone());
        Ok(())
    }

    async fn set_ipv4_forwarding(&mut self, enabled: bool) -> Res<()> {
        self.ipv4_forwarding_calls.push(enabled);
        Ok(())
    }

    async fn set_ipv6_forwarding(&mut self, enabled: bool) -> Res<()> {
        self.ipv6_forwarding_calls.push(enabled);
        Ok(())
    }

    async fn set_ipv6_disable(&mut self, disable: bool) -> Res<()> {
        self.ipv6_disable_calls.push(disable);
        Ok(())
    }

    async fn set_rp_filter(&mut self, value: u8) -> Res<()> {
        self.rp_filter_calls.push(value);
        Ok(())
    }
}

impl TcxAttach for MockLink {
    type Program = MockProgram;

    fn upsert_tcx_program(
        &mut self,
        prog: &mut MockProgram,
        bpffs_dir: impl AsRef<Path>,
        attach_type: TcAttachType,
    ) -> Res<PinnedTcxProgram> {
        let _ = bpffs_dir;
        if let Some(netns) = &self.netns {
            return Err(InfraError::TcxRequiresLocalLink {
                ifname: self.ifname.clone(),
                netns: netns.clone(),
            });
        }
        self.tcx_upsert_calls.push((prog.name.clone(), attach_type));
        self.next_link_id += 1;
        Ok(PinnedTcxProgram {
            name: prog.name.clone(),
            link_id: self.next_link_id,
        })
    }

    fn has_tcx_link(&mut self, program: &str, attach_type: TcAttachType) -> Res<bool> {
        self.tcx_has_link_calls
            .push((program.to_string(), attach_type));
        Ok(self.has_tcx_link_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MacAddress, MockNetworkProvisioner, NetworkProvisioner, Res, VethPair, VethSpec};

    async fn provision_and_start_port<P: NetworkProvisioner>(
        provisioner: &mut P,
        peer_netns: &str,
    ) -> Res<VethPair<P::LinkType>> {
        let mut pair = provisioner
            .create_veth(VethSpec {
                host_ifname: "veth-test0".to_owned(),
                peer_ifname: "veth-test1".to_owned(),
                peer_netns: Netns::path_for(peer_netns),
                host_mac: Some(MacAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])),
                peer_mac: None,
            })
            .await?;
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

    #[test]
    fn upsert_tcx_program_rejects_non_local_link() {
        let mut link = MockLink {
            netns: Some(PathBuf::from("some-ns")),
            ..Default::default()
        };
        let mut prog = MockProgram {
            name: "prog".to_owned(),
        };

        let err = link
            .upsert_tcx_program(&mut prog, "/tmp", TcAttachType::Egress)
            .expect_err("attaching to a non-local mock link should fail");
        assert!(matches!(err, InfraError::TcxRequiresLocalLink { .. }));
        assert!(link.tcx_upsert_calls.is_empty());
    }

    #[test]
    fn upsert_tcx_program_allows_local_link() {
        let mut link = MockLink::default();
        let mut prog = MockProgram {
            name: "prog".to_owned(),
        };

        link.upsert_tcx_program(&mut prog, "/tmp", TcAttachType::Egress)
            .expect("attaching to a local mock link should succeed");
        assert_eq!(
            link.tcx_upsert_calls,
            vec![("prog".to_owned(), TcAttachType::Egress)]
        );
    }
}
