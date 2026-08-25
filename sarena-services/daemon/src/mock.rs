use async_trait::async_trait;
use sarena_api_types_v1::daemon::DaemonConfigurationResponse;

use crate::{DaemonService, Res};

#[derive(Debug, Default, Clone, Copy)]
pub struct MockDaemonService;

#[async_trait]
impl DaemonService for MockDaemonService {
    async fn config(&self) -> Res<DaemonConfigurationResponse> {
        Ok(DaemonConfigurationResponse {
            device_mtu: 9000,
            route_mtu: 9000,
        })
    }

    async fn health(&self) -> Res<()> {
        Ok(())
    }
}
