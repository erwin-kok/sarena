use crate::maps::{EndpointMap, GlobalMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hook {
    TcxIngress,
    TcxEgress,
}

#[derive(Clone, Copy, Debug)]
pub struct HookSpec {
    pub hook: Hook,
    pub program_name: &'static str,
    pub required: bool,
}

pub(crate) const CONTAINER_HOOKS: &[HookSpec] = &[
    HookSpec {
        hook: Hook::TcxIngress,
        program_name: "from_container",
        required: true,
    },
    HookSpec {
        hook: Hook::TcxEgress,
        program_name: "to_container",
        required: true,
    },
];

pub(crate) const HOST_HOOKS: &[HookSpec] = &[
    HookSpec {
        hook: Hook::TcxEgress,
        program_name: "from_host",
        required: true,
    },
    HookSpec {
        hook: Hook::TcxIngress,
        program_name: "to_host",
        required: true,
    },
];

pub(crate) const NETDEV_HOOKS: &[HookSpec] = &[
    HookSpec {
        hook: Hook::TcxIngress,
        program_name: "from_netdev",
        required: true,
    },
    HookSpec {
        hook: Hook::TcxEgress,
        program_name: "to_netdev",
        required: true,
    },
];

pub(crate) const OVERLAY_HOOKS: &[HookSpec] = &[
    HookSpec {
        hook: Hook::TcxIngress,
        program_name: "from_overlay",
        required: true,
    },
    HookSpec {
        hook: Hook::TcxEgress,
        program_name: "to_overlay",
        required: true,
    },
];

pub(crate) const WIREGUARD_HOOKS: &[HookSpec] = &[
    HookSpec {
        hook: Hook::TcxIngress,
        program_name: "from_wireguard",
        required: true,
    },
    HookSpec {
        hook: Hook::TcxEgress,
        program_name: "to_wireguard",
        required: true,
    },
];

pub(crate) const CONTAINER_PER_ENDPOINT_MAPS: &[EndpointMap] =
    &[EndpointMap::CallsMap, EndpointMap::EndpointConfigMap];
pub(crate) const HOST_PER_ENDPOINT_MAPS: &[EndpointMap] =
    &[EndpointMap::CallsMap, EndpointMap::EndpointConfigMap];
pub(crate) const NETDEV_PER_ENDPOINT_MAPS: &[EndpointMap] = &[EndpointMap::CallsMap];
pub(crate) const OVERLAY_PER_ENDPOINT_MAPS: &[EndpointMap] = &[EndpointMap::CallsMap];
pub(crate) const WIREGUARD_PER_ENDPOINT_MAPS: &[EndpointMap] = &[EndpointMap::CallsMap];

pub(crate) const GLOBAL_MAPS: &[GlobalMap] = &[GlobalMap::LxcMap];
