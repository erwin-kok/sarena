use std::sync::Arc;

use sarena_services_daemon::DaemonService;
use sarena_services_endpoint::EndpointService;
use sarena_services_ipam::IpamService;

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
