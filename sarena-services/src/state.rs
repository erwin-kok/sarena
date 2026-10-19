use std::sync::Arc;

use crate::{DaemonService, EndpointService, IpamService};

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
