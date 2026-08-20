use std::{
    collections::HashMap,
    fs,
    net::{IpAddr, Ipv4Addr},
    path::Path,
    sync::LazyLock,
};

use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    routing::{delete, get, put},
};
use aya::maps::{Array, Map, MapData, hash_map};
use sarena_api_types_v1::{daemon, endpoint, ipam};
use sarena_infra::{
    InterfaceAddress, Link as _, MacAddress, NetlinkNetworkProvisioner, NetworkProvisioner,
    netlink_link::NetlinkLink,
};
use sarena_loader::{AyaBackend, EndpointHandle, EndpointKind, Loader, LoaderHandle, PinRoot};
use sarena_shared::{EndpointConfig, EndpointInfo, Ipv4Key, Ipv4KeyExt as _};
use tokio::net::UnixListener;
use tracing::info;

static POD_IPS: LazyLock<HashMap<&'static str, Ipv4Addr>> = LazyLock::new(|| {
    HashMap::from([
        ("demo/pod1", Ipv4Addr::new(192, 168, 1, 10)),
        ("demo/pod2", Ipv4Addr::new(192, 168, 2, 20)),
    ])
});

const GATEWAY_IP: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 5);

const PIN_ROOT: &str = "/sys/fs/bpf/sarena";

#[derive(Debug)]
pub struct PodSpec<'a> {
    pub name: &'a str, // human label for logging only
    pub container_id: &'a str,
    pub netns_name: &'a str, // name under `ip netns`
    pub netns_path: &'a str, // /var/run/netns/<netnsName>, what gets passed as CNI_NETNS
    pub if_name: &'a str,
    pub k8s_ns: &'a str,
    pub k8s_pod: &'a str,
    pub k8s_uid: &'a str,
}

impl<'a> PodSpec<'a> {
    pub fn cni_args_string(&self) -> String {
        format!(
            "IgnoreUnknown=1;K8S_POD_NAMESPACE={};K8S_POD_NAME={};K8S_POD_INFRA_CONTAINER_ID={};K8S_POD_UID={}",
            self.k8s_ns, self.k8s_pod, self.container_id, self.k8s_uid
        )
    }
}

#[derive(Clone)]
pub struct AppState {
    pub loader_handle: LoaderHandle,
    pub netlink_provisioner: NetlinkNetworkProvisioner,
}

impl AppState {
    pub fn new(
        loader_handle: LoaderHandle,
        netlink_provisioner: NetlinkNetworkProvisioner,
    ) -> Self {
        Self {
            loader_handle,
            netlink_provisioner,
        }
    }
}

pub struct FakeApiServer;

impl Default for FakeApiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeApiServer {
    pub fn new() -> Self {
        Self {}
    }

    pub fn start(&self, driver_sock: &str) {
        let _ = fs::remove_dir_all(PIN_ROOT);

        std::fs::create_dir_all(format!("{PIN_ROOT}/globals")).expect("creating globals dir");

        let dir = std::env::var("EBPF_DIR").unwrap_or_else(|_| "/usr/lib/sarena/ebpf".into());
        let backend = AyaBackend::new(
            format!("{dir}/sarena-ebpf-programs.o"),
            format!("{PIN_ROOT}/globals"),
        );
        let loader = Loader::new(backend, PIN_ROOT);
        let loader_handle = LoaderHandle::spawn(loader, 16);

        let provisioner = NetlinkNetworkProvisioner;

        let state = AppState::new(loader_handle, provisioner);

        if Path::new(driver_sock).exists() {
            std::fs::remove_file(driver_sock).expect("could not remove stale socket");
            info!("Removed stale socket: {}", driver_sock);
        }

        let unix_listener = UnixListener::bind(driver_sock).expect("could not bind unix listener");
        info!("Listening on Unix domain socket {}", driver_sock);

        let unix_app = Self::build_router(state);

        tokio::spawn(async move {
            if let Err(e) = axum::serve(unix_listener, unix_app).await {
                tracing::error!("Unix socket server error: {:?}", e);
            }
        });
    }

    fn build_router(state: AppState) -> Router {
        Router::new()
            .nest(
                sarena_api_types_v1::DEFAULT_BASE_PATH,
                Router::new()
                    .route("/daemon/config", get(get_config))
                    .route("/daemon/health", get(get_health))
                    .route("/endpoint/{attachment_id}", put(create_endpoint))
                    .route("/endpoint/{attachment_id}", delete(delete_endpoint))
                    .route("/endpoint/{attachment_id}/health", get(endpoint_health))
                    .route("/ipam", put(allocate_ip)),
            )
            .with_state(state)
    }
}

pub type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

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
        ipv4: ipv4,
        ipv6: None,
    };

    info!("allocate ip response: {:?}", response);

    Ok(Json(response))
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
