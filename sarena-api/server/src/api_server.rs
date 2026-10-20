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
use metrics_exporter_prometheus::PrometheusHandle;
use sarena_services::AppState;
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
        prometheus_handle: PrometheusHandle,
        shutdown: CancellationToken,
    ) -> anyhow::Result<()> {
        let router = build_router(state, prometheus_handle);

        tokio::try_join!(
            unix_listener(driver_sock, router.clone(), shutdown.child_token()),
            tcp_listener(tcp_port, router, shutdown.child_token()),
        )?;

        info!("API server shut down");
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
                let io = TokioIo::new(UnixStreamCompat(stream));
                let conn = serve_connection(io, router.clone(), shutdown.clone());
                connections.spawn(async move {
                    if let Err(err) = conn.await {
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
                let io = TokioIo::new(stream);
                let conn = serve_connection(io, router.clone(), shutdown.clone());
                connections.spawn(async move {
                    if let Err(err) = conn.await {
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

async fn serve_connection<I>(
    io: I,
    router: Router,
    shutdown: CancellationToken,
) -> hyper::Result<()>
where
    I: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let service = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
        router.clone().oneshot(req)
    });
    let conn = http1::Builder::new().serve_connection(io, service);
    tokio::pin!(conn);

    tokio::select! {
        result = conn.as_mut() => result,
        () = shutdown.cancelled() => {
            conn.as_mut().graceful_shutdown();
            conn.await
        }
    }
}

fn build_router(state: AppState, prometheus_handle: PrometheusHandle) -> Router {
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
        .merge(handlers::metrics::routes(prometheus_handle))
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
