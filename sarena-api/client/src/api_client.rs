use std::{env, sync::Arc, time::Duration};

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode, Uri, header};
use serde::{Serialize, de::DeserializeOwned};
use tokio::time::sleep;
use tracing::debug;
use uuid::Uuid;

use crate::{
    daemon::DaemonClient,
    endpoint::EndpointClient,
    error::{Res, TransportError},
    ipam::IpamClient,
    transport::{TcpTransport, Transport, TransportKind, UnixTransport},
};

const DEFAULT_SOCKET_PATH: &str = "/tmp/sarena.sock";
const X_REQUEST_ID: &str = "x-request-id";
const DEFAULT_RETRY_ATTEMPTS: u32 = 3;
const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub interval: Duration,
}

impl RetryPolicy {
    pub const fn new(max_attempts: u32, interval: Duration) -> Self {
        Self {
            max_attempts,
            interval,
        }
    }

    /// Fail on the first error, no retries.
    pub const fn none() -> Self {
        Self::new(1, Duration::ZERO)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(DEFAULT_RETRY_ATTEMPTS, DEFAULT_RETRY_INTERVAL)
    }
}

pub struct ApiClient<T: Transport + 'static> {
    daemon: Arc<DaemonClient<T>>,
    ipam: Arc<IpamClient<T>>,
    endpoint: Arc<EndpointClient<T>>,
}

impl<T: Transport + 'static> ApiClient<T> {
    pub fn new(transport: T, host: &str, base_path: &str, retry: RetryPolicy) -> Res<Self> {
        let base_uri = format!("{host}{base_path}").parse::<Uri>()?;
        let inner = Arc::new(ApiClientInner {
            transport,
            base_uri,
            retry,
        });

        let daemon = DaemonClient::new(Arc::clone(&inner));
        let ipam = IpamClient::new(Arc::clone(&inner));
        let endpoint = EndpointClient::new(Arc::clone(&inner));

        Ok(Self {
            daemon,
            ipam,
            endpoint,
        })
    }

    pub fn daemon(&self) -> &DaemonClient<T> {
        &self.daemon
    }

    pub fn ipam(&self) -> &IpamClient<T> {
        &self.ipam
    }

    pub fn endpoint(&self) -> &EndpointClient<T> {
        &self.endpoint
    }
}

impl ApiClient<TransportKind> {
    pub fn new_client_with_retry(
        raw_host: Option<String>,
        retry: RetryPolicy,
    ) -> Res<ApiClient<TransportKind>> {
        let raw_host = raw_host.unwrap_or_else(default_socket_path_protocol);

        let (scheme, host) = raw_host
            .split_once("://")
            .ok_or_else(|| TransportError::InvalidHost(raw_host.clone()))?;

        match scheme.to_lowercase().as_str() {
            "unix" => {
                debug!("Using UNIX: {}", host);
                let transport = TransportKind::Unix(UnixTransport::new(host));
                ApiClient::new(
                    transport,
                    "http://localhost",
                    sarena_api_types_v1::DEFAULT_BASE_PATH,
                    retry,
                )
            }
            "tcp" => {
                let host = format!("http://{host}");
                debug!("Using TCP: {}", host);
                let transport = TransportKind::Tcp(TcpTransport::default());
                ApiClient::new(
                    transport,
                    &host,
                    sarena_api_types_v1::DEFAULT_BASE_PATH,
                    retry,
                )
            }
            _ => Err(TransportError::InvalidScheme(scheme.to_string())),
        }
    }

    pub fn new_default_client() -> Res<ApiClient<TransportKind>> {
        Self::new_client_with_retry(None, RetryPolicy::default())
    }
}

pub struct ApiClientInner<T: Transport + 'static> {
    transport: T,
    base_uri: Uri,
    retry: RetryPolicy,
}

impl<T: Transport> ApiClientInner<T> {
    pub(crate) async fn get_api_data<U>(&self, endpoint: &str) -> Res<U>
    where
        U: DeserializeOwned,
    {
        let resp = self.send_and_check(Method::GET, endpoint, None).await?;
        serde_json::from_slice(resp.body()).map_err(TransportError::Json)
    }

    pub(crate) async fn put_api_data<U, V>(&self, endpoint: &str, data: &V) -> Res<U>
    where
        U: DeserializeOwned,
        V: Serialize,
    {
        let body = serde_json::to_vec(data)
            .map(Bytes::from)
            .map_err(TransportError::Json)?;
        let resp = self
            .send_and_check(Method::PUT, endpoint, Some(body))
            .await?;
        serde_json::from_slice(resp.body()).map_err(TransportError::Json)
    }

    async fn send_and_check(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Bytes>,
    ) -> Res<Response<Bytes>> {
        let attempts = self.retry.max_attempts.max(1);
        let mut attempt = 1;
        loop {
            match self.try_send(method.clone(), endpoint, body.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(e) if attempt < attempts && is_retryable(&e) => {
                    debug!(endpoint, attempt, error = %e, "retrying after transient error");
                    sleep(self.retry.interval).await;
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn try_send(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Bytes>,
    ) -> Res<Response<Bytes>> {
        let req = self.request(method, endpoint, body)?;
        let resp = self.transport.send(req).await?;

        if !resp.status().is_success() {
            return Err(status_error(endpoint, resp));
        }

        Ok(resp)
    }

    pub(crate) fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Bytes>,
    ) -> Res<Request<Bytes>> {
        Ok(Request::builder()
            .method(method)
            .uri(self.join_uri(path)?)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_TYPE, "application/json")
            .header(X_REQUEST_ID, Uuid::new_v4().to_string())
            .body(body.unwrap_or_default())?)
    }

    fn join_uri(&self, path: &str) -> Res<Uri> {
        let base = self.base_uri.path().trim_end_matches('/');
        let path = path.trim_start_matches('/');

        Ok(Uri::builder()
            .scheme(self.base_uri.scheme_str().unwrap_or("http"))
            .authority(self.base_uri.authority().map_or("", |a| a.as_str()))
            .path_and_query(format!("{base}/{path}"))
            .build()?)
    }
}

pub fn default_socket_path() -> String {
    env::var("SARENA_SOCKET")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_SOCKET_PATH.to_string())
}

pub fn default_socket_path_protocol() -> String {
    format!("unix://{}", default_socket_path())
}

fn status_error(endpoint: &str, resp: Response<Bytes>) -> TransportError {
    let status = resp.status();
    if status == StatusCode::NOT_FOUND {
        return TransportError::ResourceNotExist(endpoint.to_string());
    }
    TransportError::UnexpectedStatus {
        endpoint: endpoint.to_string(),
        status,
        body: String::from_utf8_lossy(resp.body()).into_owned(),
    }
}

fn is_retryable(err: &TransportError) -> bool {
    match err {
        TransportError::NetworkError(_) => true,
        TransportError::UnexpectedStatus { status, .. } => status.is_server_error(),
        _ => false,
    }
}
