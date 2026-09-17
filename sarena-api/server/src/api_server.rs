use std::path::Path;

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
use prometheus::Registry;
use sarena_control_plane::AppState;
use tokio::net::{TcpListener, UnixListener};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tower_http::trace::{MakeSpan, TraceLayer};
use tracing::info;
use uuid::Uuid;

use crate::{handlers, unix_stream::UnixStreamCompat};

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

    pub async fn start(
        &self,
        driver_sock: &str,
        tcp_port: u16,
        state: AppState,
        metrics_registry: Registry,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        let router = build_router(state, metrics_registry);

        let unix_router = router.clone();
        let unix_shutdown = shutdown.child_token();
        let unix_driver_sock = driver_sock.to_owned();

        let tcp_router = router;
        let tcp_shutdown = shutdown.child_token();

        let mut unix = tokio::spawn(async move {
            unix_listener(&unix_driver_sock, unix_router, unix_shutdown).await
        });

        let mut tcp =
            tokio::spawn(async move { tcp_listener(tcp_port, tcp_router, tcp_shutdown).await });

        tokio::select! {
            biased;

            () = shutdown.cancelled() => {
                info!("API server shutting down");
            }

            result = &mut unix => {
                result??;
                anyhow::bail!("Unix API server exited unexpectedly");
            }

            result = &mut tcp => {
                result??;
                anyhow::bail!("TCP API server exited unexpectedly");
            }
        }

        // Wait for both listener tasks to finish draining in-flight connections.
        unix.await??;
        tcp.await??;

        Ok(())
    }
}

async fn unix_listener(
    driver_sock: &str,
    router: Router,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    if Path::new(driver_sock).exists() {
        std::fs::remove_file(driver_sock)?;
        info!("Removed stale socket: {}", driver_sock);
    }

    let listener = UnixListener::bind(driver_sock)?;
    info!("Listening on Unix domain socket {}", driver_sock);

    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
             () = shutdown.cancelled() => {
                info!("Unix API listener shutting down");
                break;
            }

            result = listener.accept() => {
                let (stream, _peer_addr) = result?;
                let router = router.clone();
                connections.spawn(async move {
                    let io = TokioIo::new(UnixStreamCompat(stream));
                    let service =
                        hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                            router.clone().oneshot(req)
                        });

                    if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                        tracing::error!(
                            ?err,
                            "Unix connection closed with error"
                        );
                    }
                });
            }

            Some(result) = connections.join_next() => {
                if let Err(err) = result {
                    tracing::error!(
                        ?err,
                        "Unix connection task failed"
                    );
                }
            }
        }
    }

    // No more accepts. Wait for active connections.
    while let Some(result) = connections.join_next().await {
        if let Err(err) = result {
            tracing::error!(?err, "Unix connection task failed during shutdown");
        }
    }

    // Remove our socket rather than leaving it behind.
    let _ = std::fs::remove_file(driver_sock);

    Ok(())
}

async fn tcp_listener(
    tcp_port: u16,
    router: Router,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", tcp_port)).await?;
    info!("Listening on TCP 127.0.0.1:{}", tcp_port);

    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                info!("TCP API listener shutting down");
                break;
            }

            result = listener.accept() => {
                let (stream, peer) = result?;
                let router = router.clone();
                connections.spawn(async move {
                let io = TokioIo::new(stream);
                    let service =
                        hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                            router.clone().oneshot(req)
                        });

                    if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                        tracing::error!(
                            %peer,
                            ?err,
                            "TCP HTTP connection closed with error"
                        );
                    }
                });
            }

            Some(result) = connections.join_next() => {
                if let Err(err) = result {
                    tracing::error!(
                        ?err,
                        "HTTP connection task failed"
                    );
                }
            }
        }
    }

    // No more accepts. Wait for active connections.
    while let Some(result) = connections.join_next().await {
        if let Err(err) = result {
            tracing::error!(?err, "HTTP connection task failed during shutdown");
        }
    }

    Ok(())
}

fn build_router(state: AppState, metrics_registry: Registry) -> Router {
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
        .merge(handlers::metrics::routes(metrics_registry))
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
