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
        hook: Hook::TcxIngress,
        program_name: "from_host",
        required: true,
    },
    HookSpec {
        hook: Hook::TcxEgress,
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
    // Example of where XDP acceleration would slot in later:
    // HookSpec { hook: Hook::Xdp, program_name: "xdp_netdev", required: false },
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

pub(crate) const CONTAINER_PER_ENDPOINT_MAPS: &[&str] = &["calls_map", "endpoint_config"];
pub(crate) const HOST_PER_ENDPOINT_MAPS: &[&str] = &["calls_map", "endpoint_config"];
pub(crate) const NETDEV_PER_ENDPOINT_MAPS: &[&str] = &["calls_map"];
pub(crate) const OVERLAY_PER_ENDPOINT_MAPS: &[&str] = &["calls_map"];
pub(crate) const WIREGUARD_PER_ENDPOINT_MAPS: &[&str] = &["calls_map"];
