use std::{fs, path::Path};

use axum::{
    Router,
    body::Body,
    extract::Request,
    middleware::{self, Next},
    response::Response,
};
use http::{HeaderName, HeaderValue};
use hyper::{body::Incoming, server::conn::http1};
use hyper_util::rt::TokioIo;
use sarena_infra::NetlinkNetworkProvisioner;
use sarena_loader::{AyaBackend, Loader, LoaderHandle};
use tokio::net::{TcpListener, UnixListener};
use tower::ServiceExt;
use tower_http::trace::{MakeSpan, TraceLayer};
use tracing::info;
use uuid::Uuid;

use crate::{
    handlers::{self, endpoint::PIN_ROOT},
    state::AppState,
    unix_stream::UnixStreamCompat,
};

const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

pub struct ApiServer;

impl Default for ApiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiServer {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn start(&self, driver_sock: &str, tcp_port: u16) -> anyhow::Result<()> {
        let _ = fs::remove_dir_all(PIN_ROOT);

        std::fs::create_dir_all(format!("{PIN_ROOT}/globals")).expect("creating globals dir");

        let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
        let backend = AyaBackend::new(
            format!("{dir}/sarena-ebpf-programs.o"),
            format!("{PIN_ROOT}/globals"),
        );
        let loader = Loader::new(backend, PIN_ROOT);
        let loader_handle = LoaderHandle::spawn(loader, 16);

        let provisioner = NetlinkNetworkProvisioner;

        let state = AppState::new(loader_handle, provisioner);
        let app = build_router(state);

        unix_listener(driver_sock, app.clone())?;
        tcp_listener(tcp_port, app).await?;

        Ok(())
    }
}

fn unix_listener(driver_sock: &str, router: Router) -> anyhow::Result<()> {
    if Path::new(driver_sock).exists() {
        std::fs::remove_file(driver_sock)?;
        info!("Removed stale socket: {}", driver_sock);
    }

    let unix_listener = UnixListener::bind(driver_sock)?;
    info!("Listening on Unix domain socket {}", driver_sock);

    tokio::spawn(async move {
        loop {
            match unix_listener.accept().await {
                Ok((stream, _peer_addr)) => {
                    let io = TokioIo::new(UnixStreamCompat(stream));
                    let router = router.clone();
                    tokio::spawn(async move {
                        let service =
                            hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                                router.clone().oneshot(req)
                            });

                        if let Err(err) = http1::Builder::new().serve_connection(io, service).await
                        {
                            tracing::error!("Unix socket connection error: {:?}", err);
                        }
                    });
                }
                Err(e) => tracing::error!("Unix socket accept error: {:?}", e),
            }
        }
    });

    Ok(())
}

async fn tcp_listener(tcp_port: u16, router: Router) -> anyhow::Result<()> {
    let tcp_listener = TcpListener::bind(("127.0.0.1", tcp_port)).await?;
    info!("Listening on TCP 127.0.0.1:{}", tcp_port);

    tokio::spawn(async move {
        loop {
            match tcp_listener.accept().await {
                Ok((stream, _peer_addr)) => {
                    let io = TokioIo::new(stream);
                    let router = router.clone();
                    tokio::spawn(async move {
                        let service =
                            hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                                router.clone().oneshot(req)
                            });

                        if let Err(err) = http1::Builder::new().serve_connection(io, service).await
                        {
                            tracing::error!("TCP connection error: {:?}", err);
                        }
                    });
                }
                Err(e) => tracing::error!("TCP accept error: {:?}", e),
            }
        }
    });

    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .nest(
            sarena_api_types_v1::DEFAULT_BASE_PATH,
            Router::new()
                .nest("/daemon", handlers::daemon::routes())
                .nest("/endpoint", handlers::endpoint::routes())
                .nest("/ipam", handlers::ipam::routes()),
        )
        .layer(TraceLayer::new_for_http().make_span_with(HttpRequestSpan))
        .layer(middleware::from_fn(request_id_middleware))
        .with_state(state)
}

#[derive(Clone, Debug)]
pub struct RequestId(pub String);

async fn request_id_middleware(mut req: Request<Body>, next: Next) -> Response {
    let request_id = req
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|v| v.to_str().ok())
        .map_or_else(|| Uuid::new_v4().to_string(), ToString::to_string);

    req.extensions_mut().insert(RequestId(request_id.clone()));

    let mut response = next.run(req).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(&X_REQUEST_ID, value);
    }

    response
}

#[derive(Clone)]
struct HttpRequestSpan;

impl<B> MakeSpan<B> for HttpRequestSpan {
    fn make_span(&mut self, request: &Request<B>) -> tracing::Span {
        let request_id = request
            .extensions()
            .get::<RequestId>()
            .map_or("unknown", |r| r.0.as_str());

        tracing::info_span!(
            "http_request",
            method     = %request.method(),
            uri        = %request.uri(),
            request_id,
        )
    }
}
