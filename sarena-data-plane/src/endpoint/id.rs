use core::fmt;
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

use crate::endpoint::EndpointId;

pub enum EndpointIdentification {
    ContainerId(String),
    ContainerName(String),
    DockerEndpoint(String),
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Local(EndpointId),
    PodName(String),
}

impl fmt::Display for EndpointIdentification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContainerId(v) => write!(f, "container_id:{v}"),
            Self::ContainerName(v) => write!(f, "container_name:{v}"),
            Self::DockerEndpoint(v) => write!(f, "docker_endpoint:{v}"),
            Self::Ipv4(v) => write!(f, "ipv4:{v}"),
            Self::Ipv6(v) => write!(f, "ipv6:{v}"),
            Self::Local(v) => write!(f, "local:{v}"),
            Self::PodName(v) => write!(f, "pod_name:{v}"),
        }
    }
}

impl FromStr for EndpointIdentification {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, value) = s
            .split_once(':')
            .ok_or_else(|| "Expected a type:value string".to_string())?;

        match kind {
            "container_id" => Ok(Self::ContainerId(value.to_string())),
            "container_name" => Ok(Self::ContainerName(value.to_string())),
            "docker_endpoint" => Ok(Self::DockerEndpoint(value.to_string())),
            "ipv4" => value
                .parse()
                .map(Self::Ipv4)
                .map_err(|e| format!("Invalid IPv4 address: {e}")),
            "ipv6" => value
                .parse()
                .map(Self::Ipv6)
                .map_err(|e| format!("Invalid IPv6 address: {e}")),
            "local" => value
                .parse::<EndpointId>()
                .map(Self::Local)
                .map_err(|e| format!("Invalid local endpoint ID: {e}")),
            "pod_name" => Ok(Self::PodName(value.to_string())),
            _ => Err(format!("Unknown endpoint identification type: {kind}")),
        }
    }
}
