use std::{fs, path::Path};

use aya::{
    Ebpf,
    programs::{SchedClassifier, TcAttachType},
};
use sarena_infra::{
    InfraError, Link, NetlinkNetworkProvisioner, Netns, NetworkProvisioner, TcxAttach, VethSpec,
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
        let netns = Netns::open(&ns).expect("open temp netns");
        let lo = provisioner.get_link_in_ns(&netns, "lo").await.unwrap();

        // Attaching the same program twice should result in a link create,
        // then an update -- both must resolve to the same kernel link ID.
        let first = upsert_tcx(&lo, program, link_dir, TcAttachType::Egress).unwrap();
        let second = upsert_tcx(&lo, program, link_dir, TcAttachType::Egress).unwrap();
        assert_eq!(first.link_id, second.link_id);

        assert!(has_tcx(&lo, "dummy_test", TcAttachType::Egress).unwrap());

        second.detach(link_dir).unwrap();

        assert!(!has_tcx(&lo, "dummy_test", TcAttachType::Egress).unwrap());

        fs::remove_dir_all(link_dir).unwrap();
    })
    .await;
}

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn upsert_tcx_recovers_from_a_defunct_pin_after_device_replacement() {
    let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
    let mut test_bpf = Ebpf::load_file(format!("{dir}/sarena-ebpf-test-programs.o")).unwrap();
    let program = test_bpf.program_mut("dummy_test").unwrap();
    let program: &mut SchedClassifier = program.try_into().unwrap();
    program.load().unwrap();

    test_support::with_temp_netns("dpi-enolink-", |ns| async move {
        let link_dir = Path::new("/sys/fs/bpf/sarena-test-tcx-enolink");
        let _ = fs::remove_dir_all(link_dir);
        fs::create_dir_all(link_dir).unwrap();

        let mut provisioner = NetlinkNetworkProvisioner;

        let mut pair = provisioner
            .create_veth(VethSpec {
                host_ifname: test_support::unique_name("dpienolk0-"),
                peer_ifname: test_support::unique_name("dpienolk1-"),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");

        let first = upsert_tcx(&pair.host, program, link_dir, TcAttachType::Egress)
            .expect("first-ever attach should succeed");

        // Delete the device *without* detaching first - this is exactly
        // what leaves the pinned link defunct instead of cleanly removed.
        provisioner
            .delete_veth(&mut pair)
            .await
            .expect("delete_veth failed");

        // An unrelated replacement device to reattach onto, reusing the
        // same pin path/program name.
        let mut pair2 = provisioner
            .create_veth(VethSpec {
                host_ifname: test_support::unique_name("dpienolk2-"),
                peer_ifname: test_support::unique_name("dpienolk3-"),
                peer_netns: ns.clone(),
                host_mac: None,
                peer_mac: None,
            })
            .await
            .expect("create_veth failed");

        let second = upsert_tcx(&pair2.host, program, link_dir, TcAttachType::Egress).expect(
            "re-attaching after the old device was replaced should succeed, not fail with EEXIST",
        );

        assert_ne!(
            first.link_id, second.link_id,
            "the defunct link must have been replaced by a genuinely new one, not reused"
        );
        assert!(has_tcx(&pair2.host, "dummy_test", TcAttachType::Egress).unwrap());

        second.detach(link_dir).unwrap();
        provisioner
            .delete_veth(&mut pair2)
            .await
            .expect("cleanup delete_veth failed");
        fs::remove_dir_all(link_dir).unwrap();
    })
    .await;
}

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
