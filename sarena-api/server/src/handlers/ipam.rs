use std::{collections::HashMap, net::Ipv4Addr, sync::LazyLock};

use axum::{
    Json, Router,
    extract::State,
    routing::{delete, put},
};
use http::StatusCode;
use sarena_api_types_v1::ipam;
use tracing::info;

use crate::{error::ApiResult, state::AppState};

static POD_IPS: LazyLock<HashMap<&'static str, Ipv4Addr>> = LazyLock::new(|| {
    HashMap::from([
        ("demo/pod1", Ipv4Addr::new(192, 168, 1, 10)),
        ("demo/pod2", Ipv4Addr::new(192, 168, 2, 20)),
    ])
});

const GATEWAY_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 5);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", put(allocate_ip))
        .route("/", delete(delete_ip))
}

pub async fn allocate_ip(
    State(_state): State<AppState>,
    Json(params): Json<ipam::IpamAllocateRequest>,
) -> ApiResult<ipam::IpamAllocateResponse> {
    info!("allocate ip: {:?}", params);

    let ipv4 = POD_IPS
        .get(params.owner.as_str())
        .copied()
        .map(|ip| ipam::ContainerAddressing {
            ip: ip.to_string(),
            pool: None,
        });

    let response = ipam::IpamAllocateResponse {
        host_addressing: ipam::HostAddressing {
            ipv4: Some(GATEWAY_IP.to_string()),
            ipv6: None,
        },
        ipv4,
        ipv6: None,
    };

    info!("allocate ip response: {:?}", response);

    Ok(Json(response))
}

pub async fn delete_ip(
    State(_state): State<AppState>,
    Json(params): Json<ipam::IpamReleaseRequest>,
) -> StatusCode {
    info!("release ip: {:?}", params);

    StatusCode::NO_CONTENT
}
