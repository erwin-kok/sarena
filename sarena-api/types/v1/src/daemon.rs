use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct DaemonConfigurationResponse {
    #[serde(rename = "deviceMTU")]
    pub device_mtu: u32,

    #[serde(rename = "routeMTU")]
    pub route_mtu: u32,
}
