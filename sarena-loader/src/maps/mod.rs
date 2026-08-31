pub mod calls_map;
pub mod endpoint_config_map;
pub mod lxc_map;

pub use calls_map::CallsMap;
pub use endpoint_config_map::EndpointConfigMap;
pub use lxc_map::LxcMap;

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlobalMap {
    LxcMap,
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
