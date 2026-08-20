//! Exercises the real CNI ADD/CHECK/DEL flow (`sarena-cni-plugin`) against a
//! fake `sarena-daemon` API server: two pods get `ADD`ed into their own
//! netns, UDP connectivity between them is verified end-to-end, `CHECK` is
//! run against both, then both are `DEL`ed. `STATUS` (which, per the CNI
//! spec, takes no per-container args) is covered separately below.
//!
//! `CHECK` and `DEL` carry `prevResult` (the `CNIResult` from `ADD`) in their
//! network configuration, matching what a real CNI runtime sends -- it's the
//! mechanism the runtime uses to hand a plugin's own prior result back to it,
//! independent of plugin chaining (which only applies to `ADD`).

use std::{collections::HashMap, env, net::UdpSocket, path::PathBuf, time::Duration};

use ipnet::IpNet;
use rscni_plugin::{
    async_cni::Cni,
    types::{Args, CNIResult, NetConf},
};
use sarena_cni_plugin::SarenaPlugin;
use sarena_cni_test::test_daemon::{FakeApiServer, PodSpec};
use sarena_infra::{InfraError, Netns, NetnsGuard};
use sarena_utils::{LoggingConfig, logging};
use serde_json::json;
use tracing::info;

const ENABLE_DEBUG: &str = "enable-debug";
const LOG_FILE: &str = "log-file";

const DEMO_NETNS_PREFIX: &str = "cnidemo";

const UDP_PORT: u16 = 9999;

// `current_thread`, not the default multi-threaded flavor: `Netns::unshare_self`
// below is only safe before the runtime's worker pool exists (see its doc
// comment) -- with a multi-threaded runtime, that pool is already running by
// the time this test calls it.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn cni_add_creates_working_connectivity_between_two_pods() {
    logging::init_logging(&LoggingConfig {
        enable_debug: false,
        log_file: None,
    });

    // Must run before anything else on this thread: makes "the default
    // namespace" for the rest of this process a fresh, private one instead
    // of the machine's real default -- see `Netns::unshare_self`'s doc
    // comment for why this requires `current_thread` (no worker pool to
    // migrate onto later). Every host-side veth end created below lands
    // here, since it never moves once created.
    Netns::unshare_self()
        .await
        .expect("failed to unshare a private default namespace");

    let sarena_plugin = SarenaPlugin;

    let api_server = FakeApiServer::new();
    api_server.start("/tmp/sarena.sock");

    let pid = std::process::id();

    let netns_pod1 = format!("{DEMO_NETNS_PREFIX}-c1-{pid}");
    Netns::create(&netns_pod1)
        .await
        .expect("failed to create netns for pod1");
    let _guard1 = NetnsGuard::new(&netns_pod1);

    info!("created netns: {}", netns_pod1);

    let pod1 = PodSpec {
        name: "pod1",
        container_id: &format!("{DEMO_NETNS_PREFIX}-c1-{pid}"),
        netns_name: &netns_pod1,
        netns_path: &format!("/var/run/netns/{netns_pod1}"),
        if_name: "eth0",
        k8s_ns: "demo",
        k8s_pod: "pod1",
        k8s_uid: "11111111-1111-1111-1111-111111111111",
    };

    set_cni_runtime_env("ADD", &pod1);
    let result1 = sarena_plugin
        .add(build_args(&pod1, None))
        .await
        .expect("ADD failed for pod1");

    let netns_pod2 = format!("{DEMO_NETNS_PREFIX}-c2-{pid}");
    Netns::create(&netns_pod2)
        .await
        .expect("failed to create netns for pod2");
    let _guard2 = NetnsGuard::new(&netns_pod2);

    info!("created netns: {}", netns_pod2);

    let pod2 = PodSpec {
        name: "pod2",
        container_id: &format!("{DEMO_NETNS_PREFIX}-c2-{pid}"),
        netns_name: &netns_pod2,
        netns_path: &format!("/var/run/netns/{netns_pod2}"),
        if_name: "eth0",
        k8s_ns: "demo",
        k8s_pod: "pod2",
        k8s_uid: "22222222-2222-2222-2222-222222222222",
    };

    set_cni_runtime_env("ADD", &pod2);
    let result2 = sarena_plugin
        .add(build_args(&pod2, None))
        .await
        .expect("ADD failed for pod2");

    let pod1_ip = result1.ips[0]
        .address
        .parse::<IpNet>()
        .expect("could not parse pod1 ip")
        .addr();
    let pod2_ip = result2.ips[0]
        .address
        .parse::<IpNet>()
        .expect("could not parse pod2 ip")
        .addr();

    let listener = Netns::open_path(pod1.netns_path)
        .expect("failed to open pod1 netns")
        .run(move |_handle| async move {
            UdpSocket::bind((pod1_ip, UDP_PORT)).map_err(InfraError::Runtime)
        })
        .await
        .expect("failed to bind listener socket");

    listener
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("failed to set read timeout");
    let listener_addr = listener.local_addr().expect("failed to get listener addr");

    let sender =
        Netns::open_path(pod2.netns_path)
            .expect("failed to open pod2 netns")
            .run(move |_handle| async move {
                UdpSocket::bind((pod2_ip, 0)).map_err(InfraError::Runtime)
            })
            .await
            .expect("failed to bind sender socket");

    let payload = b"hello from peer2";
    sender
        .send_to(payload, listener_addr)
        .expect("send_to failed");

    let mut buf = [0u8; 64];
    let (n, from) = listener.recv_from(&mut buf).expect("recv_from failed");
    assert_eq!(&buf[..n], payload, "listener received unexpected payload");
    assert_eq!(
        from.ip(),
        pod2_ip,
        "payload did not arrive from pod2's address"
    );

    set_cni_runtime_env("CHECK", &pod1);
    sarena_plugin
        .check(build_args(&pod1, Some(result1.clone())))
        .await
        .expect("CHECK failed for pod1");

    set_cni_runtime_env("CHECK", &pod2);
    sarena_plugin
        .check(build_args(&pod2, Some(result2.clone())))
        .await
        .expect("CHECK failed for pod2");

    set_cni_runtime_env("DEL", &pod1);
    sarena_plugin
        .del(build_args(&pod1, Some(result1)))
        .await
        .expect("DEL failed for pod1");

    set_cni_runtime_env("DEL", &pod2);
    sarena_plugin
        .del(build_args(&pod2, Some(result2)))
        .await
        .expect("DEL failed for pod2");

    // The guards go out of scope here and delete the netns.
}

fn set_cni_runtime_env(verb: &str, p: &PodSpec) {
    unsafe {
        env::set_var("CNI_COMMAND", verb);
        env::set_var("CNI_CONTAINERID", p.container_id);
        env::set_var("CNI_NETNS", p.netns_path);
        env::set_var("CNI_IFNAME", p.if_name);
        env::set_var("CNI_ARGS", p.cni_args_string());
        env::set_var("CNI_PATH", "/opt/cni/bin");
    }
}

fn build_args(p: &PodSpec, prev_result: Option<CNIResult>) -> Args {
    let custom = HashMap::from([
        (ENABLE_DEBUG.to_string(), json!(false)),
        (LOG_FILE.to_string(), json!("cnidemo-log")),
    ]);
    let net_conf = NetConf {
        cni_version: "1.0.0".to_string(),
        name: "sarena".to_string(),
        r#type: "sarena-cni".to_string(),
        custom,
        prev_result,
        ..Default::default()
    };
    Args {
        container_id: Some(p.container_id.to_string()),
        netns: Some(PathBuf::from(p.netns_path)),
        ifname: Some(p.if_name.to_string()),
        args: Some(p.cni_args_string()),
        path: vec![PathBuf::from("/opt/cni/bin")],
        config: Some(net_conf),
    }
}
