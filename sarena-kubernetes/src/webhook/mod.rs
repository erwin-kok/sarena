mod address_pool;

use std::{net::SocketAddr, time::Duration};

use axum::{Router, routing::post};
use axum_server::{Handle, tls_rustls::RustlsConfig};
use kube::Client;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error};

/// Namespace the webhook `Service` and TLS `Secret` are expected to live in.
pub const NAMESPACE: &str = "sarena-system";
/// Name of the `Service` fronting the webhook server.
pub const SERVICE_NAME: &str = "sarena-kubernetes-webhook";
/// Name of the `Secret` holding the webhook server's TLS certificate and key.
pub const SECRET_NAME: &str = "sarena-kubernetes-webhook-tls";
/// Path the `AddressPool` validating webhook is served on.
pub const ADDRESS_POOL_VALIDATE_PATH: &str = "/validate/addresspool";

const DEFAULT_ADDR: &str = "0.0.0.0:8443";
const DEFAULT_TLS_CERT_PATH: &str = "/etc/sarena/webhook/tls.crt";
const DEFAULT_TLS_KEY_PATH: &str = "/etc/sarena/webhook/tls.key";
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const TLS_RETRY_INTERVAL: Duration = Duration::from_secs(5);

pub async fn run(_client: Client, shutdown: CancellationToken) {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    let addr = std::env::var("SARENA_WEBHOOK_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let addr: SocketAddr = match addr.parse() {
        Ok(addr) => addr,
        Err(err) => {
            error!(%addr, %err, "invalid SARENA_WEBHOOK_ADDR, webhook server disabled");
            shutdown.cancelled().await;
            return;
        }
    };

    let cert_path = std::env::var("SARENA_WEBHOOK_TLS_CERT")
        .unwrap_or_else(|_| DEFAULT_TLS_CERT_PATH.to_string());
    let key_path = std::env::var("SARENA_WEBHOOK_TLS_KEY")
        .unwrap_or_else(|_| DEFAULT_TLS_KEY_PATH.to_string());

    let Some(tls_config) = load_tls_config(&cert_path, &key_path, &shutdown).await else {
        debug!("webhook server shutting down before TLS certificate became available");
        return;
    };

    let app = Router::new().route(ADDRESS_POOL_VALIDATE_PATH, post(address_pool::validate));

    let handle = Handle::new();
    tokio::spawn({
        let handle = handle.clone();
        async move {
            shutdown.cancelled().await;
            handle.graceful_shutdown(Some(GRACEFUL_SHUTDOWN_TIMEOUT));
        }
    });

    debug!(%addr, "starting webhook server");

    if let Err(err) = axum_server::bind_rustls(addr, tls_config)
        .handle(handle)
        .serve(app.into_make_service())
        .await
    {
        error!(%err, "webhook server failed");
    }

    debug!("webhook server stopped");
}

async fn load_tls_config(
    cert_path: &str,
    key_path: &str,
    shutdown: &CancellationToken,
) -> Option<RustlsConfig> {
    let mut attempt: u32 = 0;

    loop {
        match RustlsConfig::from_pem_file(cert_path, key_path).await {
            Ok(tls_config) => {
                if attempt > 0 {
                    debug!(
                        cert_path,
                        key_path, "loaded webhook TLS certificate after retry"
                    );
                }
                return Some(tls_config);
            }
            Err(err) => {
                if attempt == 0 {
                    error!(
                        cert_path,
                        key_path,
                        %err,
                        "failed to load webhook TLS certificate, will retry"
                    );
                } else {
                    debug!(cert_path, key_path, %err, "still no webhook TLS certificate");
                }
            }
        }

        attempt += 1;

        tokio::select! {
            () = tokio::time::sleep(TLS_RETRY_INTERVAL) => {}
            () = shutdown.cancelled() => return None,
        }
    }
}
