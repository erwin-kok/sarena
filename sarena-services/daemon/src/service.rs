use std::env::consts;

use async_trait::async_trait;
use sarena_api_types_v1::daemon::{
    DaemonConfigurationResponse, DaemonDebugInfoResponse, SarenaVersion,
};

use crate::{DaemonService, Res};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("SARENA_CLI_GIT_HASH");
const BUILD_DATE: &str = env!("SARENA_CLI_BUILD_DATE");

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

    async fn debuginfo(&self) -> Res<DaemonDebugInfoResponse> {
        Ok(DaemonDebugInfoResponse {
            version: SarenaVersion {
                version: VERSION.to_owned(),
                git_hash: GIT_HASH.to_owned(),
                build_date: BUILD_DATE.to_owned(),
                os: consts::OS.to_owned(),
                arch: consts::ARCH.to_owned(),
            },
        })
    }
}
