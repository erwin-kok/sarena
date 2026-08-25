use axum::{Json, Router, extract::State, routing::get};
use sarena_api_types_v1::daemon;

use crate::{
    error::{ApiResult, ApiStatus, Res},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_config))
        .route("/health", get(get_health))
}

pub async fn get_config(
    State(state): State<AppState>,
) -> ApiResult<daemon::DaemonConfigurationResponse> {
    let response = state.daemon.config().await?;
    Ok(Json(response))
}

pub async fn get_health(State(state): State<AppState>) -> Res<ApiStatus> {
    state.daemon.health().await?;
    Ok(ApiStatus::Ok)
}
