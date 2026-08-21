use axum::{Json, Router, extract::State, routing::get};
use hyper::StatusCode;
use sarena_api_types_v1::daemon;

use crate::{error::ApiResult, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_config))
        .route("/health", get(get_health))
}

pub async fn get_config(
    State(_state): State<AppState>,
) -> ApiResult<daemon::DaemonConfigurationResponse> {
    Ok(Json(daemon::DaemonConfigurationResponse {
        device_mtu: 1500,
        route_mtu: 1500,
    }))
}

pub async fn get_health(State(_state): State<AppState>) -> StatusCode {
    StatusCode::OK
}
