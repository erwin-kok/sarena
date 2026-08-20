use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IpamAllocateRequest {
    #[serde(rename = "owner")]
    pub owner: String,

    #[serde(rename = "pool", skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IpamAllocateResponse {
    #[serde(rename = "host-addressing")]
    pub host_addressing: HostAddressing,

    #[serde(rename = "ipv4", skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<ContainerAddressing>,

    #[serde(rename = "ipv6", skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<ContainerAddressing>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct HostAddressing {
    #[serde(rename = "ipv4", skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,

    #[serde(rename = "ipv6", skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct ContainerAddressing {
    #[serde(rename = "ip")]
    pub ip: String,

    #[serde(rename = "pool", skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct IpamReleaseRequest {
    #[serde(rename = "ip")]
    pub ip: String,

    #[serde(rename = "pool", skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}
