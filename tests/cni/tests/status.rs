//! Exercises CNI `STATUS` against a fake `sarena-daemon` API server.
//!
//! Per the CNI spec, `STATUS` takes no per-container args (no
//! `CNI_CONTAINERID`/`CNI_NETNS`/`CNI_IFNAME`) -- it's a plugin/daemon
//! readiness check, not an operation on a specific pod, so unlike
//! `end_to_end.rs` this needs neither a pod netns nor connectivity checking.
//!
//! This lives in its own test binary, not alongside `end_to_end.rs`: both
//! start a `FakeApiServer` bound to the same fixed `/tmp/sarena.sock` (the
//! path `ApiClient::new_default_client()` always connects to) and pin the
//! same eBPF program under the same `PIN_ROOT`, so running them concurrently
//! -- as cargo would with multiple `#[test]` fns in one binary -- would
//! collide. Splitting into separate files, run sequentially by the
//! justfile's `_root-test` loop, avoids that.

use std::{collections::HashMap, env};

use rscni_plugin::{
    async_cni::Cni,
    types::{Args, NetConf},
};
use sarena_cni_plugin::SarenaPlugin;
use sarena_cni_test::test_daemon::FakeApiServer;
use sarena_infra::Netns;
use sarena_utils::{LoggingConfig, logging};
use serde_json::json;

const ENABLE_DEBUG: &str = "enable-debug";
const LOG_FILE: &str = "log-file";

// `current_thread`, not the default multi-threaded flavor: `Netns::unshare_self`
// below is only safe before the runtime's worker pool exists (see its doc
// comment) -- with a multi-threaded runtime, that pool is already running by
// the time this test calls it.
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn cni_status_reports_daemon_ready() {
    logging::init_logging(&LoggingConfig {
        enable_debug: false,
        log_file: None,
    });

    Netns::unshare_self()
        .await
        .expect("failed to unshare a private default namespace");

    let sarena_plugin = SarenaPlugin;

    let api_server = FakeApiServer::new();
    api_server.start("/tmp/sarena.sock");

    unsafe {
        env::set_var("CNI_COMMAND", "STATUS");
        env::set_var("CNI_PATH", "/opt/cni/bin");
    }

    sarena_plugin
        .status(status_args())
        .await
        .expect("STATUS failed");
}

fn status_args() -> Args {
    let custom = HashMap::from([
        (ENABLE_DEBUG.to_string(), json!(false)),
        (LOG_FILE.to_string(), json!("cnidemo-log")),
    ]);
    let net_conf = NetConf {
        cni_version: "1.0.0".to_string(),
        name: "sarena".to_string(),
        r#type: "sarena-cni".to_string(),
        custom,
        ..Default::default()
    };
    Args {
        container_id: None,
        netns: None,
        ifname: None,
        args: None,
        path: vec!["/opt/cni/bin".into()],
        config: Some(net_conf),
    }
}
