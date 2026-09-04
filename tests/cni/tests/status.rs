use std::env;

use rscni_plugin::{async_cni::Cni, test_util::ArgsBuilder, types::Args};
use sarena_cni_plugin::SarenaPlugin;
use sarena_cni_test::test_daemon::FakeApiServer;
use sarena_infra::Netns;
use sarena_utils::{LoggingConfig, logging};
use serde_json::json;

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires CAP_NET_ADMIN/CAP_SYS_ADMIN and a writable /run/netns"]
async fn cni_status_reports_daemon_ready() {
    logging::init_logging(&LoggingConfig::default());

    Netns::unshare_self()
        .await
        .expect("failed to unshare a private default namespace");

    let sarena_plugin = SarenaPlugin;

    let api_server = FakeApiServer::new();
    api_server.start("/tmp/sarena.sock").await;

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
    let config = json!({
        "cniVersion": "1.0.0",
        "name": "sarena",
        "type": "sarena-cni",
        "enable-debug": false,
        "log-file": "cnidemo-log"
    });
    ArgsBuilder::new()
        .path("/opt/cni/bin")
        .config(&config.to_string())
        .expect("valid network config")
        .build()
        .expect("failed to build Args")
}
