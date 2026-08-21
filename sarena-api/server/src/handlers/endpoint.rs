use std::net::{IpAddr, Ipv4Addr};

use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    routing::{delete, get, put},
};
use aya::maps::{Array, Map, MapData, hash_map};
use sarena_api_types_v1::endpoint;
use sarena_infra::{
    InterfaceAddress, Link as _, MacAddress, NetworkProvisioner, netlink_link::NetlinkLink,
};
use sarena_loader::{EndpointHandle, EndpointKind, PinRoot};
use sarena_shared::{EndpointConfig, EndpointInfo, Ipv4Key, Ipv4KeyExt as _};
use tracing::info;

use crate::{error::ApiResult, state::AppState};

pub const PIN_ROOT: &str = "/sys/fs/bpf/sarena";

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
    info!("create endpoint {attachment_id}: {:?}", ep);

    let host = state
        .netlink_provisioner
        .get_link(&ep.host_iface_name)
        .await
        .expect("get_link");

    let handle = state
        .loader_handle
        .add_endpoint(EndpointKind::Container, &ep.host_iface_name)
        .await
        .expect("add endpoint");

    if let Some(ipv4) = ep.ipv4 {
        let peer_ip: IpAddr = ipv4.ip.parse::<InterfaceAddress>().expect("parse ip").ip;
        let peer_ip = match peer_ip {
            IpAddr::V4(ipv4_addr) => ipv4_addr,
            IpAddr::V6(_) => panic!("IPv6 peer addresses are not supported"),
        };

        let host_mac = MacAddress::parse(&ep.host_mac).expect("parse host mac");
        set_endpoint_config(&handle, host_mac, peer_ip);

        let peer_mac = MacAddress::parse(&ep.container_mac).expect("parse container mac");
        insert_endpoint_info(peer_ip, &host, peer_mac);
    }

    Ok(Json(endpoint::EndpointCreateResponse {}))
}

fn set_endpoint_config(handle: &EndpointHandle, host_mac: MacAddress, peer_ip: Ipv4Addr) {
    let path = &handle.map_paths["endpoint_config"];
    let map_data = MapData::from_pin(path).expect("map from_pin");
    let map = Map::Array(map_data);
    let mut array: Array<_, EndpointConfig> = Array::try_from(map).expect("try_from");
    array
        .set(
            0,
            EndpointConfig {
                mac: host_mac.0,
                ipv4: peer_ip,
            },
            0,
        )
        .expect("setting element");
}

fn insert_endpoint_info(peer_ip: Ipv4Addr, link: &NetlinkLink, peer_mac: MacAddress) {
    let pin_root = PinRoot::new(PIN_ROOT);
    let path = pin_root.global_map_dir("lxc_map");
    let map_data = MapData::from_pin(path).expect("map from_pin");
    let map = Map::from_map_data(map_data).expect("from_map_data");

    let mut lxc_map: hash_map::HashMap<_, Ipv4Key, EndpointInfo> =
        hash_map::HashMap::try_from(map).expect("try_from");

    let key = Ipv4Key::from_addr(peer_ip);
    let value = EndpointInfo {
        if_index: link.ifindex(),
        mac: peer_mac.0,
    };
    lxc_map.insert(key, value, 0).expect("insert element");
}

pub async fn delete_endpoint(
    State(_state): State<AppState>,
    AxumPath(attachment_id): AxumPath<String>,
) -> StatusCode {
    info!("delete endpoint {attachment_id}");

    StatusCode::NO_CONTENT
}

pub async fn endpoint_health(
    State(_state): State<AppState>,
    AxumPath(attachment_id): AxumPath<String>,
) -> ApiResult<endpoint::EndpointHealthResponse> {
    info!("endpoint health {attachment_id}");

    Ok(Json(endpoint::EndpointHealthResponse {
        heatlh: endpoint::EndpointHealthStatus::Ok,
    }))
}
