use core::fmt;

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

impl fmt::Display for SarenaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, built {}, {}/{})",
            self.version, self.git_hash, self.build_date, self.os, self.arch
        )
    }
}
