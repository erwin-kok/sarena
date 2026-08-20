use std::future::Future;

use async_trait::async_trait;
use rscni_plugin::{
    async_cni::Cni,
    error::Error,
    types::{Args, CNIResult, NetConf},
};
use sarena_utils::{LoggingConfig, logging};
use serde_json::Value;
use tracing::{Instrument as _, Span, debug, debug_span};
use uuid::Uuid;

use crate::{
    Res,
    args::{ArgsSpec, load_args},
    cmd,
};

const ENABLE_DEBUG: &str = "enable-debug";
const LOG_FILE: &str = "log-file";

pub struct SarenaPlugin;

#[async_trait]
impl Cni for SarenaPlugin {
    async fn add(&self, args: Args) -> Res<CNIResult> {
        dispatch("ADD", args, cmd::add::add).await
    }

    async fn del(&self, args: Args) -> Res<CNIResult> {
        dispatch("DEL", args, cmd::del::del).await
    }

    async fn check(&self, args: Args) -> Res<CNIResult> {
        dispatch("CHECK", args, cmd::check::check).await
    }

    async fn status(&self, args: Args) -> Res<()> {
        dispatch("STATUS", args, cmd::status::status).await
    }

    async fn gc(&self, _args: Args) -> Res<()> {
        Ok(())
    }
}

async fn dispatch<F, Fut, T>(command: &'static str, args: Args, handler: F) -> Res<T>
where
    F: FnOnce(Args, ArgsSpec) -> Fut,
    Fut: Future<Output = Res<T>>,
{
    let net_conf = args
        .config()
        .ok_or_else(|| Error::InvalidNetworkConfig("failed to load netconf".to_string()))?;
    init_logging(net_conf);

    let cni_args = load_args::<ArgsSpec>(args.args.as_ref())?;
    let span = make_span(command, &args, &cni_args);

    async move {
        debug!(?args, "processing CNI {command} request");

        let result = handler(args, cni_args).await;

        debug!("CNI {command} processing complete");

        result
    }
    .instrument(span)
    .await
}

fn init_logging(net_conf: &NetConf) {
    let enable_debug = net_conf
        .custom
        .get(ENABLE_DEBUG)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let log_file = net_conf
        .custom
        .get(LOG_FILE)
        .and_then(|v| v.as_str())
        .map(String::from);
    logging::init_logging(&LoggingConfig {
        enable_debug,
        log_file,
    });
}

fn make_span(command: &'static str, args: &Args, cni_args: &ArgsSpec) -> Span {
    debug_span!(
        "cni_request",
        command,
        event_id = %Uuid::new_v4(),
        container_id = ?args.container_id,
        netns = ?args.netns,
        interface = ?args.ifname,
        k8s_namespace = %cni_args.k8s_pod_namespace,
        k8s_pod = %cni_args.k8s_pod_name,
    )
}
