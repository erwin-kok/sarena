use axum::{Router, body::Body, extract::State, response::Response, routing::get};
use http::{StatusCode, header};
use metrics_exporter_prometheus::PrometheusHandle;

pub fn routes(prometheus_handle: PrometheusHandle) -> Router {
    Router::new()
        .route("/metrics", get(get_metrics))
        .with_state(prometheus_handle)
}

async fn get_metrics(State(prometheus_handle): State<PrometheusHandle>) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )
        .body(Body::from(prometheus_handle.render()))
        .expect("valid metrics response")
}
