use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    routing::{delete, get, put},
};
use sarena_api_types_v1::endpoint;

use crate::{
    error::{ApiResult, ApiStatus, Res},
    state::AppState,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{attachment_id}", put(create_endpoint))
        .route("/{attachment_id}", delete(delete_endpoint))
        .route("/{attachment_id}/health", get(endpoint_health))
}

pub async fn create_endpoint(
    State(state): State<AppState>,
    AxumPath(attachment_id): AxumPath<String>,
    Json(ep): Json<endpoint::EndpointCreateRequest>,
) -> ApiResult<endpoint::EndpointCreateResponse> {
    let response = state.endpoint.create(attachment_id, ep).await?;
    Ok(Json(response))
}

pub async fn delete_endpoint(
    State(state): State<AppState>,
    AxumPath(attachment_id): AxumPath<String>,
) -> Res<ApiStatus> {
    state.endpoint.delete(attachment_id).await?;
    Ok(ApiStatus::NoContent)
}

pub async fn endpoint_health(
    State(state): State<AppState>,
    AxumPath(attachment_id): AxumPath<String>,
) -> ApiResult<endpoint::EndpointHealthResponse> {
    let response = state.endpoint.health(attachment_id).await?;
    Ok(Json(response))
}
