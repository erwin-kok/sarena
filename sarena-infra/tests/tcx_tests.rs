use std::{fs, path::Path};

use aya::{
    Ebpf,
    programs::{SchedClassifier, TcAttachType},
};
use sarena_infra::{
    InfraError, Link, NetlinkNetworkProvisioner, NetworkProvisioner, TcxAttach, VethSpec,
    tcx::{has_tcx, upsert_tcx},
    test_support,
};

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn attach_detach_tcx() {
    let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
    let mut test_bpf = Ebpf::load_file(format!("{dir}/sarena-ebpf-test-programs.o")).unwrap();
    let program = test_bpf.program_mut("dummy_test").unwrap();
    let program: &mut SchedClassifier = program.try_into().unwrap();
    program.load().unwrap();

    test_support::with_temp_netns("dpi-xdp-", |ns| async move {
        let link_dir = Path::new("/sys/fs/bpf/sarena-test-attach-detach-tcx");
        let _ = fs::remove_dir_all(link_dir);
        fs::create_dir_all(link_dir).unwrap();

        let provisioner = NetlinkNetworkProvisioner;
        let lo = provisioner.get_link_in_ns(&ns, "lo").await.unwrap();

        // Attaching the same program twice should result in a link create,
        // then an update -- both must resolve to the same kernel link ID.
        let first = upsert_tcx(&lo, program, link_dir, TcAttachType::Egress).unwrap();
        let second = upsert_tcx(&lo, program, link_dir, TcAttachType::Egress).unwrap();
        assert_eq!(first.link_id, second.link_id);

        assert!(has_tcx(&lo, &second, TcAttachType::Egress).unwrap());

        second.detach(link_dir).unwrap();

        assert!(!has_tcx(&lo, &second, TcAttachType::Egress).unwrap());

        fs::remove_dir_all(link_dir).unwrap();
    })
    .await;
}

/// `TcxAttach::upsert_tcx_program`'s implementations should refuse to
/// attach to a link that isn't in the caller's own namespace -- a program
/// pinned from here wouldn't be attached where the caller thinks it is.
/// The free `tcx::upsert_tcx_program` function used by `attach_detach_tcx`
/// above doesn't carry this guard itself (it has no way to know a device's
/// namespace); it's enforced one layer up, in the `TcxAttach` trait impl.
#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn upsert_tcx_program_rejects_non_local_link() {
    let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
    let mut test_bpf = Ebpf::load_file(format!("{dir}/sarena-ebpf-test-programs.o")).unwrap();
    let program = test_bpf.program_mut("dummy_test").unwrap();
    let program: &mut SchedClassifier = program.try_into().unwrap();
    program.load().unwrap();

    test_support::with_temp_netns("dpi-tcxns-", |ns| async move {
        let link_dir = Path::new("/sys/fs/bpf/sarena-test-tcx-reject");
        let _ = fs::remove_dir_all(link_dir);
        fs::create_dir_all(link_dir).unwrap();

        let mut provisioner = NetlinkNetworkProvisioner;
        let host_name = test_support::unique_name("dpitcx0-");
        let peer_name = test_support::unique_name("dpitcx1-");
        let pair = provisioner
            .create_veth(VethSpec {
                host_ifname: host_name.clone(),
                peer_ifname: peer_name.clone(),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");
        let (mut host, mut peer) = (pair.host, pair.peer);

        // The peer end was moved into `ns` by `create_veth` -- not the
        // caller's own namespace -- so this must be refused.
        let err = peer
            .upsert_tcx_program(program, link_dir, TcAttachType::Egress)
            .expect_err("attaching to a link outside the caller's own namespace should fail");
        assert!(matches!(err, InfraError::TcxRequiresLocalLink { .. }));
        // Nothing should have been pinned.
        assert!(fs::read_dir(link_dir).unwrap().next().is_none());

        // The host end, by contrast, is local and the same attach should
        // succeed.
        host.upsert_tcx_program(program, link_dir, TcAttachType::Egress)
            .expect("attaching to the local host end should succeed");

        fs::remove_dir_all(link_dir).unwrap();
        host.delete().await.expect("cleanup delete failed");
    })
    .await;
}
