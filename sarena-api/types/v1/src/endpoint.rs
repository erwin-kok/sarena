use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointCreateRequest {
    #[serde(rename = "container-id")]
    pub container_id: String,

    #[serde(rename = "container-interface-name")]
    pub container_iface_name: String,

    #[serde(rename = "k8s-namespace")]
    pub k8s_namespace: String,

    #[serde(rename = "k8s-pod-name")]
    pub k8s_pod_name: String,

    #[serde(rename = "k8s-uid")]
    pub k8s_uid: String,

    #[serde(rename = "mac")]
    pub container_mac: String,

    #[serde(rename = "host-mac")]
    pub host_mac: String,

    #[serde(rename = "interface-index")]
    pub host_iface_index: u32,

    #[serde(rename = "interface-name")]
    pub host_iface_name: String,

    #[serde(rename = "ipv4")]
    pub ipv4: Option<Addressing>,

    #[serde(rename = "ipv6")]
    pub ipv6: Option<Addressing>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Addressing {
    #[serde(rename = "ip")]
    pub ip: String,

    #[serde(rename = "pool", skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointCreateResponse {}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointDeleteRequest {
    #[serde(rename = "container-id")]
    pub container_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum EndpointHealthStatus {
    Ok,
    Failure,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointHealthResponse {
    #[serde(rename = "heatlh")]
    pub heatlh: EndpointHealthStatus,
}
