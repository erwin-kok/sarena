use axum::{
    Router,
    extract::State,
    http::header::CONTENT_TYPE,
    response::{IntoResponse, Response},
    routing::get,
};
use prometheus::{Encoder, Registry, TextEncoder};

pub fn routes(registry: Registry) -> Router {
    Router::new()
        .route("/metrics", get(get_metrics))
        .with_state(registry)
}

async fn get_metrics(State(registry): State<Registry>) -> Response {
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();

    let mut buf = Vec::new();
    if let Err(err) = encoder.encode(&metric_families, &mut buf) {
        tracing::error!(?err, "failed to encode metrics");
        return (http::StatusCode::INTERNAL_SERVER_ERROR).into_response();
    }

    ([(CONTENT_TYPE, encoder.format_type())], buf).into_response()
}
