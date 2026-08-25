use async_trait::async_trait;
use sarena_api_types_v1::daemon::DaemonConfigurationResponse;

use crate::{DaemonService, Res};

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultDaemonService;

impl DefaultDaemonService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DaemonService for DefaultDaemonService {
    async fn config(&self) -> Res<DaemonConfigurationResponse> {
        Ok(DaemonConfigurationResponse {
            device_mtu: 1500,
            route_mtu: 1500,
        })
    }

    async fn health(&self) -> Res<()> {
        Ok(())
    }
}
