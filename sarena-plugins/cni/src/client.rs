use rscni_plugin::{error::Error, types::Args};
use sarena_api_client::{ApiClient, RetryPolicy, TransportKind};
use serde_json::Value;

use crate::Res;

const DAEMON_ENDPOINT: &str = "daemon-endpoint";

pub(crate) fn build_api_client(args: &Args) -> Res<ApiClient<TransportKind>> {
    let endpoint = args
        .config()
        .and_then(|c| c.custom.get(DAEMON_ENDPOINT))
        .and_then(Value::as_str)
        .map(String::from);

    ApiClient::new_client_with_retry(endpoint, RetryPolicy::default())
        .map_err(|_| Error::PluginNotAvailable("DaemonDown".to_string()))
}
