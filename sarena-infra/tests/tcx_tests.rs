use std::{fs, path::Path};

use aya::{
    Ebpf,
    programs::{SchedClassifier, TcAttachType},
};
use sarena_infra::{
    NetlinkNetworkProvisioner, NetworkProvisioner,
    tcx::{detach_tcx, has_tcx_link, upsert_tcx_program},
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
        let first = upsert_tcx_program(&lo, program, link_dir, TcAttachType::Egress).unwrap();
        let second = upsert_tcx_program(&lo, program, link_dir, TcAttachType::Egress).unwrap();
        assert_eq!(first.link_id, second.link_id);

        assert!(has_tcx_link(&lo, &second, TcAttachType::Egress).unwrap());

        detach_tcx(link_dir, &second).unwrap();

        assert!(!has_tcx_link(&lo, &second, TcAttachType::Egress).unwrap());

        fs::remove_dir_all(link_dir).unwrap();
    })
    .await;
}
