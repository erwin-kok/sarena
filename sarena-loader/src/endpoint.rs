use std::{collections::HashMap, fmt, path::PathBuf};

use crate::{
    manifest::{
        CONTAINER_HOOKS, CONTAINER_PER_ENDPOINT_MAPS, HOST_HOOKS, HOST_PER_ENDPOINT_MAPS, HookSpec,
        NETDEV_HOOKS, NETDEV_PER_ENDPOINT_MAPS, OVERLAY_HOOKS, OVERLAY_PER_ENDPOINT_MAPS,
        WIREGUARD_HOOKS, WIREGUARD_PER_ENDPOINT_MAPS,
    },
    pin::PinRoot,
};

/// Compact, daemon-allocated identifier for a container/pod endpoint.
///
/// Deliberately NOT the raw CNI/sandbox ID: the kernel caps BPF object
/// names (`BPF_OBJ_NAME_LEN`) at 16 bytes, so per-endpoint map names
/// (see `pin::PinRoot::per_endpoint_map`) need something short. A
/// short numeric id is what keeps them within budget. Allocating one
/// and mapping it to a sandbox ID is the orchestrator's job; this crate
/// just needs something small, stable and Hash+Eq.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContainerId(pub u16);

impl fmt::Display for ContainerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:05}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EndpointId {
    Container(ContainerId),
    Host(String),
    NetDev(String),
    Overlay(String),
    Wireguard(String),
}

impl EndpointId {
    /// Cheap discriminant for logging/metrics labels without a full match.
    pub fn kind_str(&self) -> &'static str {
        match self {
            EndpointId::Container(_) => "container",
            EndpointId::Host(_) => "host",
            EndpointId::NetDev(_) => "netdev",
            EndpointId::Overlay(_) => "overlay",
            EndpointId::Wireguard(_) => "wireguard",
        }
    }

    pub(crate) fn hooks(&self) -> &'static [HookSpec] {
        match self {
            EndpointId::Container(_) => CONTAINER_HOOKS,
            EndpointId::Host(_) => HOST_HOOKS,
            EndpointId::NetDev(_) => NETDEV_HOOKS,
            EndpointId::Overlay(_) => OVERLAY_HOOKS,
            EndpointId::Wireguard(_) => WIREGUARD_HOOKS,
        }
    }

    pub(crate) fn per_endpoint_map_names(&self) -> &'static [&'static str] {
        match self {
            EndpointId::Container(_) => CONTAINER_PER_ENDPOINT_MAPS,
            EndpointId::Host(_) => HOST_PER_ENDPOINT_MAPS,
            EndpointId::NetDev(_) => NETDEV_PER_ENDPOINT_MAPS,
            EndpointId::Overlay(_) => OVERLAY_PER_ENDPOINT_MAPS,
            EndpointId::Wireguard(_) => WIREGUARD_PER_ENDPOINT_MAPS,
        }
    }

    pub(crate) fn map_rename(&self, pins: &PinRoot) -> HashMap<String, PathBuf> {
        self.per_endpoint_map_names()
            .iter()
            .map(|name| ((*name).to_string(), pins.per_endpoint_map_dir(name, self)))
            .collect()
    }

    /// The network link name to resolve via `NetworkProvisioner::get_link`
    /// for this endpoint. Host/NetDev/Overlay/Wireguard endpoints already
    /// carry their ifname directly as the identifying string. A container
    /// only carries a compact numeric id (see `ContainerId`'s doc comment),
    /// so its link name is derived by a fixed convention instead - `lxc`
    /// plus the zero-padded id, e.g. `lxc00007` - short enough to stay well
    /// under `IFNAMSIZ` (15 bytes).
    pub(crate) fn link_name(&self) -> String {
        match self {
            EndpointId::Container(cid) => format!("lxc{cid}"),
            EndpointId::Host(name)
            | EndpointId::NetDev(name)
            | EndpointId::Overlay(name)
            | EndpointId::Wireguard(name) => name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_ids_are_zero_padded_and_short() {
        // keeps derived map names well under BPF_OBJ_NAME_LEN (16 bytes)
        assert_eq!(ContainerId(7).to_string(), "00007");
        assert!(ContainerId(u16::MAX).to_string().len() <= 5);
    }

    #[test]
    fn map_rename_only_lists_per_endpoint_maps() {
        let id = EndpointId::Container(ContainerId(1));
        let pins = PinRoot::new("/sys/fs/bpf/test");

        let renamed = id.map_rename(&pins);

        assert!(renamed.contains_key("calls_map"));
        assert!(
            !renamed.contains_key("shared_map"),
            "global maps aren't declared here at all"
        );
        assert_eq!(
            renamed["calls_map"],
            pins.per_endpoint_map_dir("calls_map", &id)
        );
    }

    #[test]
    fn link_name_uses_the_identifying_string_directly_except_for_containers() {
        assert_eq!(EndpointId::Host("eth0".to_string()).link_name(), "eth0");
        assert_eq!(EndpointId::NetDev("eth1".to_string()).link_name(), "eth1");
        assert_eq!(
            EndpointId::Overlay("vxlan0".to_string()).link_name(),
            "vxlan0"
        );
        assert_eq!(EndpointId::Wireguard("wg0".to_string()).link_name(), "wg0");
        assert_eq!(
            EndpointId::Container(ContainerId(7)).link_name(),
            "lxc00007"
        );
    }
}
