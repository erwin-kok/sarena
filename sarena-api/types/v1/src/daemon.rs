use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DaemonConfigurationResponse {
    #[serde(rename = "deviceMTU")]
    pub device_mtu: u32,

    #[serde(rename = "routeMTU")]
    pub route_mtu: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DaemonDebugInfoResponse {
    #[serde(rename = "version")]
    pub version: SarenaVersion,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct SarenaVersion {
    pub version: String,
    pub git_hash: String,
    pub build_date: String,
    pub os: String,
    pub arch: String,
}
