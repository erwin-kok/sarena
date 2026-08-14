use std::{collections::HashMap, path::PathBuf};

use crate::{
    manifest::{
        CONTAINER_GLOBAL_MAPS, CONTAINER_HOOKS, CONTAINER_PER_ENDPOINT_MAPS, HOST_GLOBAL_MAPS,
        HOST_HOOKS, HOST_PER_ENDPOINT_MAPS, HookSpec, NETDEV_GLOBAL_MAPS, NETDEV_HOOKS,
        NETDEV_PER_ENDPOINT_MAPS, OVERLAY_GLOBAL_MAPS, OVERLAY_HOOKS, OVERLAY_PER_ENDPOINT_MAPS,
        WIREGUARD_GLOBAL_MAPS, WIREGUARD_HOOKS, WIREGUARD_PER_ENDPOINT_MAPS,
    },
    pin::PinRoot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndpointKind {
    Container,
    Host,
    NetDev,
    Overlay,
    Wireguard,
}

impl EndpointKind {
    /// Cheap discriminant for logging/metrics labels without a full match.
    /// Also doubles as this kind's on-disk pin-path segment - see
    /// `pin::PinRoot`.
    pub fn kind_str(self) -> &'static str {
        match self {
            EndpointKind::Container => "container",
            EndpointKind::Host => "host",
            EndpointKind::NetDev => "netdev",
            EndpointKind::Overlay => "overlay",
            EndpointKind::Wireguard => "wireguard",
        }
    }

    pub(crate) fn hooks(self) -> &'static [HookSpec] {
        match self {
            EndpointKind::Container => CONTAINER_HOOKS,
            EndpointKind::Host => HOST_HOOKS,
            EndpointKind::NetDev => NETDEV_HOOKS,
            EndpointKind::Overlay => OVERLAY_HOOKS,
            EndpointKind::Wireguard => WIREGUARD_HOOKS,
        }
    }

    pub(crate) fn per_endpoint_map_names(self) -> &'static [&'static str] {
        match self {
            EndpointKind::Container => CONTAINER_PER_ENDPOINT_MAPS,
            EndpointKind::Host => HOST_PER_ENDPOINT_MAPS,
            EndpointKind::NetDev => NETDEV_PER_ENDPOINT_MAPS,
            EndpointKind::Overlay => OVERLAY_PER_ENDPOINT_MAPS,
            EndpointKind::Wireguard => WIREGUARD_PER_ENDPOINT_MAPS,
        }
    }

    pub(crate) fn global_map_names(self) -> &'static [&'static str] {
        match self {
            EndpointKind::Container => CONTAINER_GLOBAL_MAPS,
            EndpointKind::Host => HOST_GLOBAL_MAPS,
            EndpointKind::NetDev => NETDEV_GLOBAL_MAPS,
            EndpointKind::Overlay => OVERLAY_GLOBAL_MAPS,
            EndpointKind::Wireguard => WIREGUARD_GLOBAL_MAPS,
        }
    }

    pub(crate) fn map_rename(self, pins: &PinRoot, link: &str) -> HashMap<String, PathBuf> {
        self.per_endpoint_map_names()
            .iter()
            .map(|name| ((*name).to_string(), pins.per_endpoint_map_dir(name, link)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_rename_only_lists_per_endpoint_maps() {
        let pins = PinRoot::new("/sys/fs/bpf/test");

        let renamed = EndpointKind::Container.map_rename(&pins, "lxc00001");

        assert!(renamed.contains_key("calls_map"));
        assert!(
            !renamed.contains_key("shared_map"),
            "global maps aren't declared here at all"
        );
        assert_eq!(
            renamed["calls_map"],
            pins.per_endpoint_map_dir("calls_map", "lxc00001")
        );
    }
}
