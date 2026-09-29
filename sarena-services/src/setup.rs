use std::sync::Arc;

use sarena_data_plane::{DataPlaneConfig, LoaderHandle};

use crate::{
    DaemonService, DefaultDaemonService, DefaultEndpointService, DefaultIpamService,
    EndpointService, IpamService,
};

#[derive(Clone)]
pub struct AppState {
    pub ipam: Arc<dyn IpamService>,
    pub endpoint: Arc<dyn EndpointService>,
    pub daemon: Arc<dyn DaemonService>,
}

impl AppState {
    pub fn new(
        ipam: Arc<dyn IpamService>,
        endpoint: Arc<dyn EndpointService>,
        daemon: Arc<dyn DaemonService>,
    ) -> Self {
        Self {
            ipam,
            endpoint,
            daemon,
        }
    }
}

pub fn setup_services(config: DataPlaneConfig, loader_handle: LoaderHandle) -> AppState {
    let ipam = Arc::new(DefaultIpamService::new(
        config.gateway_ip,
        config.ipam_ipv4_subnet,
        config.ipam_ipv6_subnet,
    ));
    let endpoint: Arc<DefaultEndpointService> =
        Arc::new(DefaultEndpointService::new(loader_handle));
    let daemon = Arc::new(DefaultDaemonService::new());

    AppState::new(ipam, endpoint, daemon)
}
