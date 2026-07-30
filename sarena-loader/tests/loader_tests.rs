use std::{
    fs,
    path::{Path, PathBuf},
};

use aya::programs::TcAttachType;
use sarena_infra::{
    Link, NetlinkNetworkProvisioner, Netns, NetworkProvisioner, TcxAttach, VethSpec, test_support,
};
use sarena_loader::{AyaBackend, EndpointId, Loader, LoaderHandle};

const PIN_ROOT: &str = "/sys/fs/bpf/test";

#[tokio::test]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn load_multiple_links() {
    Netns::unshare_self()
        .await
        .expect("unshare_self failed -- needs CAP_NET_ADMIN");

    let _ = fs::remove_dir_all(PIN_ROOT);

    std::fs::create_dir_all(format!("{PIN_ROOT}/globals")).expect("creating globals dir");

    let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
    let backend = AyaBackend::new(
        format!("{dir}/sarena-ebpf-programs.o"),
        format!("{PIN_ROOT}/globals"),
    );
    let loader = Loader::new(backend, PIN_ROOT);
    let loader_handle = LoaderHandle::spawn(loader, 16);

    test_support::with_temp_netns("dpi-p1-", |peer1_ns| async move {
        test_support::with_temp_netns("dpi-p2-", |peer2_ns| async move {
            let mut provisioner = NetlinkNetworkProvisioner;

            let host1_name = test_support::unique_name("dpir10-");
            let peer1_name = test_support::unique_name("dpir11-");
            let host2_name = test_support::unique_name("dpir20-");
            let peer2_name = test_support::unique_name("dpir21-");

            // `create_veth` moves the peer end into `peer{1,2}_ns` as part
            // of creation; the host end stays in the default namespace.
            let pair1 = provisioner
                .create_veth(VethSpec {
                    host_ifname: host1_name.clone(),
                    peer_ifname: peer1_name.clone(),
                    peer_netns: peer1_ns.clone(),
                    host_mac: None,
                    peer_mac: None,
                })
                .await
                .expect("create_veth (pair 1) failed");
            let mut host1 = pair1.host;

            let pair2 = provisioner
                .create_veth(VethSpec {
                    host_ifname: host2_name.clone(),
                    peer_ifname: peer2_name.clone(),
                    peer_netns: peer2_ns.clone(),
                    host_mac: None,
                    peer_mac: None,
                })
                .await
                .expect("create_veth (pair 2) failed");
            let mut host2 = pair2.host;

            loader_handle
                .add_endpoint(EndpointId::Host(host1_name.clone()))
                .await
                .expect("add_endpoint (host1) failed");
            loader_handle
                .add_endpoint(EndpointId::Host(host2_name.clone()))
                .await
                .expect("add_endpoint (host2) failed");

            assert!(
                host1
                    .has_tcx_link("from_host", TcAttachType::Ingress)
                    .expect("expect program")
            );
            assert!(
                host1
                    .has_tcx_link("to_host", TcAttachType::Egress)
                    .expect("expect program")
            );

            let pins = collect_pins(Path::new(PIN_ROOT));
            for host_name in [&host1_name, &host2_name] {
                for prog_name in ["from_host", "to_host"] {
                    let expected = PathBuf::from(format!("links/host/{host_name}/{prog_name}"));
                    assert!(
                        pins.contains(&expected),
                        "missing link pin {expected:?}, found: {pins:?}"
                    );
                }
            }

            assert!(
                pins.contains(&PathBuf::from("globals/interface_config")),
                "missing map pin \"globals/interface_config\", found: {pins:?}"
            );

            for host_name in [&host1_name, &host2_name] {
                let expected = PathBuf::from(format!("globals/calls_map_{host_name}"));
                assert!(
                    pins.contains(&expected),
                    "missing map pin {expected:?}, found: {pins:?}"
                );
            }

            loader_handle.teardown_all().await.expect("teardown failed");

            host1.delete().await.expect("host1 cleanup delete failed");
            host2.delete().await.expect("host2 cleanup delete failed");
        })
        .await;
    })
    .await;
}

fn collect_pins(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_dir(dir, dir, &mut out);
    out
}

fn walk_dir(dir: &Path, prefix: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, prefix, out);
        } else if let Ok(rel) = path.strip_prefix(prefix) {
            out.push(rel.to_path_buf());
        }
    }
}
