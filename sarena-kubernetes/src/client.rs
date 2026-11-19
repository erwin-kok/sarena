//! Connecting to the Kubernetes API server.

use std::{
    env,
    path::PathBuf,
    time::{Duration, Instant},
};

use http::{HeaderValue, Request, StatusCode, header::USER_AGENT};
use k8s_openapi::api::core::v1::Namespace;
use kube::{
    Api, Client, Config,
    config::{KubeConfigOptions, Kubeconfig},
};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::error::{KubernetesError, Res};

const USER_AGENT_VALUE: &str = concat!("Sarena/", env!("CARGO_PKG_VERSION"));

/// Default for [`ClientOptions::heartbeat_timeout`].
pub const DEFAULT_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long [`connect`] keeps retrying to reach the API server.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// How long [`connect`] waits between attempts.
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_secs(5);

/// Namespace whose existence [`connect`] checks to see whether the API server
/// is reachable and we're authorized.
const READINESS_NAMESPACE: &str = "kube-system";

/// How to reach the API server, see [`create_config`].
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// API server URL, e.g. `https://10.0.0.1:6443`.
    pub api_server_url: Option<String>,
    /// Path of a kubeconfig file.
    pub kubeconfig_path: Option<PathBuf>,
    /// Bounds connecting to and waiting for a response from the API server,
    /// and the interval and timeout of [`run_heartbeat`]. `None` disables
    /// both: requests may then hang on a dead connection.
    pub heartbeat_timeout: Option<Duration>,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            api_server_url: None,
            kubeconfig_path: None,
            heartbeat_timeout: Some(DEFAULT_HEARTBEAT_TIMEOUT),
        }
    }
}

/// Whether there is any Kubernetes cluster to connect to: `options` names
/// one, a kubeconfig exists (`$KUBECONFIG` or `~/.kube/config`), or we run
/// inside a pod (`KUBERNETES_SERVICE_HOST` is set).
///
/// Only checks for presence: whether that configuration is valid and the
/// API server reachable is up to [`connect`].
pub fn is_configured(options: &ClientOptions) -> bool {
    let env_set = |name| env::var_os(name).is_some_and(|value| !value.is_empty());

    options.api_server_url.is_some()
        || options.kubeconfig_path.is_some()
        || env_set("KUBECONFIG")
        || env::home_dir().is_some_and(|home| home.join(".kube").join("config").is_file())
        || env_set("KUBERNETES_SERVICE_HOST")
}

/// Builds the client configuration. In order of precedence:
///
/// 1. `kubeconfig_path`: that kubeconfig file (its current context).
/// 2. an `https://` `api_server_url`: the in-cluster configuration (service account token and CA)
///    but with that URL.
/// 3. any other `api_server_url`: that URL, without authentication.
/// 4. neither: inferred, i.e. `$KUBECONFIG` or `~/.kube/config` when present, the in-cluster
///    configuration otherwise.
pub async fn create_config(options: &ClientOptions) -> Res<Config> {
    let mut config = match (&options.kubeconfig_path, &options.api_server_url) {
        (Some(path), _) => {
            let kubeconfig = Kubeconfig::read_from(path)?;
            Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default()).await?
        }
        (None, Some(url)) => {
            let cluster_url =
                url.parse()
                    .map_err(|source| KubernetesError::InvalidApiServerUrl {
                        url: url.clone(),
                        source,
                    })?;
            if url.starts_with("https://") {
                let mut config = Config::incluster()?;
                config.cluster_url = cluster_url;
                config
            } else {
                Config::new(cluster_url)
            }
        }
        (None, None) => Config::infer().await?,
    };

    config
        .headers
        .push((USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE)));

    // client-go rotates the connections of a client whose heartbeat fails, so
    // requests don't hang forever on a connection the peer silently dropped.
    // Bounding each request does the same: hyper discards a connection whose
    // request failed. Watches are covered by the watcher's own idle timeout.
    if let Some(timeout) = options.heartbeat_timeout {
        config.connect_timeout = Some(timeout);
        config.read_timeout = Some(timeout);
        // Count the read timeout from when a request is sent, not from when
        // its pooled connection went idle.
        config.reset_reader_on_write = true;
    }

    Ok(config)
}

/// Creates a client per `options` and waits until the API server answers,
/// retrying every [`CONNECT_RETRY_INTERVAL`] for up to [`CONNECT_TIMEOUT`].
pub async fn connect(options: &ClientOptions) -> Res<Client> {
    let config = create_config(options).await?;
    let host = config.cluster_url.to_string();
    let client = Client::try_from(config)?;

    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        info!(%host, "establishing connection to API server");
        match is_conn_ready(&client).await {
            Ok(()) => {
                info!(%host, "connected to API server");
                return Ok(client);
            }
            Err(err) if Instant::now() + CONNECT_RETRY_INTERVAL < deadline => {
                warn!(%host, %err, "API server not reachable yet, retrying");
                tokio::time::sleep(CONNECT_RETRY_INTERVAL).await;
            }
            Err(err) => {
                warn!(%host, %err, "unable to contact API server");
                return Err(err.into());
            }
        }
    }
}

/// Whether the API server answers and lets us read namespaces.
async fn is_conn_ready(client: &Client) -> Result<(), kube::Error> {
    Api::<Namespace>::all(client.clone())
        .get(READINESS_NAMESPACE)
        .await
        .map(drop)
}

/// Checks the API server's health every `timeout` until `shutdown` is
/// cancelled, logging a warning when it doesn't answer within `timeout`.
///
/// Probes `/healthz` rather than getting this node (what kubelet does)
/// since it transfers less data.
pub async fn run_heartbeat(client: Client, timeout: Duration, shutdown: CancellationToken) {
    let mut interval = tokio::time::interval(timeout);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = interval.tick() => heartbeat(&client, timeout).await,
        }
    }
}

async fn heartbeat(client: &Client, timeout: Duration) {
    let request = match Request::get("/healthz").body(Vec::new()) {
        Ok(request) => request,
        Err(err) => {
            warn!(%err, "failed to build heartbeat request");
            return;
        }
    };

    match tokio::time::timeout(timeout, client.request_text(request)).await {
        Ok(Ok(_)) => {}
        // An overloaded API server is still reachable.
        Ok(Err(kube::Error::Api(status)))
            if status.code == StatusCode::TOO_MANY_REQUESTS.as_u16() => {}
        Ok(Err(err)) => warn!(%err, "API server heartbeat failed"),
        Err(_) => warn!(?timeout, "API server heartbeat timed out"),
    }
}
