use std::{collections::HashMap, net::IpAddr};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{delete, get, put},
};
use http::HeaderMap;
use sarena_api_types_v1::ipam;
use sarena_control_plane::AppState;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult, ApiStatus, Res};

#[derive(Debug, Deserialize)]
pub struct IpamQuery {
    family: Option<String>,
    owner: Option<String>,
    pool: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", put(allocate))
        .route("/{ip}", put(allocate_ip))
        .route("/{ip}", delete(release_ip))
        .route("/dump", get(dump))
}

pub async fn allocate(
    State(state): State<AppState>,
    Query(query): Query<IpamQuery>,
    headers: HeaderMap,
) -> ApiResult<ipam::IpamAllocateResponse> {
    let expiration = headers
        .get("expiration")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(false);
    let response = state
        .ipam
        .allocate(query.family, query.owner, query.pool, expiration)
        .await?;
    Ok(Json(response))
}

pub async fn allocate_ip(
    State(state): State<AppState>,
    Path(ip): Path<String>,
    Query(query): Query<IpamQuery>,
) -> Res<ApiStatus> {
    let ip: IpAddr = ip
        .parse()
        .map_err(|_| ApiError::bad_request(format!("Invalid IP address: {ip}")))?;
    state.ipam.allocate_ip(ip, query.owner, query.pool).await?;
    Ok(ApiStatus::Ok)
}

pub async fn release_ip(
    State(state): State<AppState>,
    Path(ip): Path<String>,
    Query(query): Query<IpamQuery>,
) -> Res<ApiStatus> {
    let ip: IpAddr = ip
        .parse()
        .map_err(|_| ApiError::bad_request(format!("Invalid IP address: {ip}")))?;
    state.ipam.release(ip, query.pool).await?;
    Ok(ApiStatus::NoContent)
}

pub async fn dump(
    State(state): State<AppState>,
) -> ApiResult<(HashMap<String, String>, HashMap<String, String>, String)> {
    let response = state.ipam.dump().await?;
    Ok(Json(response))
}
