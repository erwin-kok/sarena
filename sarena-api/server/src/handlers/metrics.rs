use axum::{
    Router, extract::State, http::header::CONTENT_TYPE, response::IntoResponse, routing::get,
};
use prometheus::{Encoder, Registry, TextEncoder};

use crate::error::{ApiError, ResultExt};

pub fn routes(registry: Registry) -> Router {
    Router::new()
        .route("/metrics", get(get_metrics))
        .with_state(registry)
}

async fn get_metrics(State(registry): State<Registry>) -> Result<impl IntoResponse, ApiError> {
    let encoder = TextEncoder::new();
    let content_type = encoder.format_type().to_owned();
    let metric_families = registry.gather();

    let mut buf = Vec::new();
    encoder.encode(&metric_families, &mut buf).internal()?;

    Ok(([(CONTENT_TYPE, content_type)], buf))
}
