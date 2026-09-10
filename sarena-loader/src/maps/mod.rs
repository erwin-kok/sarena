pub mod calls_map;
pub mod conntrack_map;
pub mod endpoint_config_map;
pub mod lxc_map;
pub mod metrics_map;

pub use calls_map::CallsMap;
pub use endpoint_config_map::EndpointConfigMap;
pub use lxc_map::LxcMap;
pub use metrics_map::MetricsMap;

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlobalMap {
    LxcMap,
    MetricsMap,
    ConnTrackTcpMap,
    ConnTrackAnyMap,
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndpointMap {
    CallsMap,
    EndpointConfigMap,
}

impl GlobalMap {
    pub const fn wire_name(self) -> &'static str {
        match self {
            GlobalMap::LxcMap => lxc_map::LXC_MAP_NAME,
            GlobalMap::MetricsMap => metrics_map::METRICS_MAP_NAME,
            GlobalMap::ConnTrackTcpMap => conntrack_map::CONNTRACK_TCP_MAP,
            GlobalMap::ConnTrackAnyMap => conntrack_map::CONNTRACK_ANY_MAP,
        }
    }
}

impl EndpointMap {
    pub const fn wire_name(self) -> &'static str {
        match self {
            EndpointMap::CallsMap => calls_map::CALLS_MAP_NAME,
            EndpointMap::EndpointConfigMap => endpoint_config_map::ENDPOINT_CONFIG_MAP_NAME,
        }
    }
}
