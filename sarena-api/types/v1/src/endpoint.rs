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
    pub ipv4: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointCreateResponse {}
