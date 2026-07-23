use std::{net::Ipv4Addr, path::Path};

use aya::programs::TcAttachType;

use crate::{Link, MacAddress, PinnedTcxProgram, Res, TcxAttach};

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
    pub addr_calls: Vec<(Ipv4Addr, u8)>,
    pub gateway_calls: Vec<Ipv4Addr>,
    pub rename_calls: Vec<String>,
    pub setns_calls: Vec<String>,
    pub delete_calls: u32,
    pub netns: Option<String>,
    pub tcx_upsert_calls: Vec<(String, TcAttachType)>,
    pub tcx_has_link_calls: Vec<(u32, TcAttachType)>,
    /// What `has_tcx_link` should answer -- set this before calling it.
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

    async fn set_ns(&mut self, target_ns: &str) -> Res<()> {
        self.setns_calls.push(target_ns.to_owned());
        self.netns = Some(target_ns.to_owned());
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

    async fn set_addr(&mut self, ip: Ipv4Addr, prefix_len: u8) -> Res<()> {
        self.addr_calls.push((ip, prefix_len));
        Ok(())
    }

    async fn add_gateway(&mut self, gateway: Ipv4Addr) -> Res<()> {
        self.gateway_calls.push(gateway);
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
        self.tcx_upsert_calls.push((prog.name.clone(), attach_type));
        self.next_link_id += 1;
        Ok(PinnedTcxProgram {
            name: prog.name.clone(),
            link_id: self.next_link_id,
        })
    }

    fn detach_tcx(_bpffs_dir: impl AsRef<Path>, _program: &PinnedTcxProgram) -> Res<()> {
        Ok(())
    }

    fn has_tcx_link(&mut self, program: &PinnedTcxProgram, attach_type: TcAttachType) -> Res<bool> {
        self.tcx_has_link_calls.push((program.link_id, attach_type));
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
                peer_netns: peer_netns.to_owned(),
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
}
