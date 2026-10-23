use std::{
    fs::{self, File},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ops::Deref,
    path::{Path, PathBuf},
};

use sarena_infra::MacAddress;
use serde::{Deserialize, Serialize};

use super::EndpointId;
use crate::Res;

pub(crate) const EP_CONFIG_NAME: &str = "ep_config.json";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct EndpointSpec {
    #[serde(rename = "container_name", skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,

    #[serde(rename = "container_id", skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,

    #[serde(rename = "docker_network_id", skip_serializing_if = "Option::is_none")]
    pub docker_network_id: Option<String>,

    #[serde(rename = "docker_endpoint", skip_serializing_if = "Option::is_none")]
    pub docker_endpoint: Option<String>,

    #[serde(rename = "interface-name")]
    pub host_iface_name: String,

    #[serde(rename = "interface-index")]
    pub host_iface_index: u32,

    #[serde(rename = "container_mac")]
    pub container_mac: MacAddress,

    #[serde(rename = "host_mac")]
    pub host_mac: MacAddress,

    #[serde(rename = "ipv4", skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<Ipv4Addr>,

    #[serde(rename = "ipv6", skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<Ipv6Addr>,

    #[serde(rename = "k8s_ns", skip_serializing_if = "Option::is_none")]
    pub k8s_ns: Option<String>,

    #[serde(rename = "k8s_pod_name", skip_serializing_if = "Option::is_none")]
    pub k8s_pod_name: Option<String>,
}

impl EndpointSpec {
    pub fn k8s_ns_and_pod_name(&self) -> Option<String> {
        Some(format!(
            "{}/{}",
            self.k8s_ns.as_deref()?,
            self.k8s_pod_name.as_deref()?
        ))
    }

    /// The IPv4 and/or IPv6 address of this endpoint.
    pub fn ips(&self) -> impl Iterator<Item = IpAddr> {
        self.ipv4
            .map(IpAddr::V4)
            .into_iter()
            .chain(self.ipv6.map(IpAddr::V6))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EndpointPersist {
    #[serde(rename = "id")]
    pub id: EndpointId,

    #[serde(flatten)]
    pub spec: EndpointSpec,
}

impl Deref for EndpointPersist {
    type Target = EndpointSpec;

    fn deref(&self) -> &EndpointSpec {
        &self.spec
    }
}

impl EndpointPersist {
    pub fn read_state(state_dir: impl Into<PathBuf>) -> Res<Self> {
        let state_dir = state_dir.into();
        let content = std::fs::read_to_string(state_dir.join(EP_CONFIG_NAME))?;
        let state: Self = serde_json::from_str(&content)?;
        Ok(state)
    }

    pub fn write_state(&self, state_dir: &Path) -> Res<()> {
        let path = state_dir.join(EP_CONFIG_NAME);
        let tmp = state_dir.join(format!("{EP_CONFIG_NAME}.tmp"));
        let file = File::create(&tmp)?;
        serde_json::to_writer_pretty(&file, self)?;
        file.sync_all()?;
        fs::rename(&tmp, &path)?;
        // The rename lives in the directory entry; sync it too, so it survives
        // a crash.
        File::open(state_dir)?.sync_all()?;
        Ok(())
    }
}
