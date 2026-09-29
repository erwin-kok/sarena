use async_trait::async_trait;
use sarena_api_types_v1::daemon::{
    DaemonConfigurationResponse, DaemonDebugInfoResponse, SarenaVersion,
};

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

    async fn debuginfo(&self) -> Res<DaemonDebugInfoResponse> {
        Ok(DaemonDebugInfoResponse {
            version: SarenaVersion {
                version: "1.0".to_string(),
                git_hash: "1.0".to_string(),
                build_date: "29/11/2026".to_string(),
                os: "Linux".to_string(),
                arch: "AMD64".to_string(),
            },
        })
    }
}
