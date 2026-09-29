use std::sync::Arc;

use sarena_control_plane::{ControlPlaneConfig, PIN_ROOT};
use sarena_data_plane::LoaderHandle;
use sarena_infra::NetlinkNetworkProvisioner;

use crate::state::AppState;

pub fn setup_services(config: ControlPlaneConfig, loader_handle: LoaderHandle) -> AppState {
    let provisioner = NetlinkNetworkProvisioner;
    let ipam = Arc::new(sarena_services_ipam::DefaultIpamService::new(
        config.gateway_ip,
        config.ipam_ipv4_subnet,
        config.ipam_ipv6_subnet,
    ));
    let endpoint: Arc<sarena_services_endpoint::DefaultEndpointService> = Arc::new(
        sarena_services_endpoint::DefaultEndpointService::new(loader_handle, provisioner, PIN_ROOT),
    );
    let daemon = Arc::new(sarena_services_daemon::DefaultDaemonService::new());

    let state = AppState::new(ipam, endpoint, daemon);

    state
}
