use crate::{
    manifest::{
        CONTAINER_HOOKS, CONTAINER_PER_ENDPOINT_MAPS, HOST_HOOKS, HOST_PER_ENDPOINT_MAPS, HookSpec,
        NETDEV_HOOKS, NETDEV_PER_ENDPOINT_MAPS, OVERLAY_HOOKS, OVERLAY_PER_ENDPOINT_MAPS,
        WIREGUARD_HOOKS, WIREGUARD_PER_ENDPOINT_MAPS,
    },
    maps::EndpointMap,
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
    pub fn kind_str(self) -> &'static str {
        match self {
            EndpointKind::Container => "container",
            EndpointKind::Host => "host",
            EndpointKind::NetDev => "netdev",
            EndpointKind::Overlay => "overlay",
            EndpointKind::Wireguard => "wireguard",
        }
    }

    pub fn from_kind_str(s: &str) -> Option<Self> {
        Some(match s {
            "container" => EndpointKind::Container,
            "host" => EndpointKind::Host,
            "netdev" => EndpointKind::NetDev,
            "overlay" => EndpointKind::Overlay,
            "wireguard" => EndpointKind::Wireguard,
            _ => return None,
        })
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

    pub(crate) fn per_endpoint_map_names(self) -> &'static [EndpointMap] {
        match self {
            EndpointKind::Container => CONTAINER_PER_ENDPOINT_MAPS,
            EndpointKind::Host => HOST_PER_ENDPOINT_MAPS,
            EndpointKind::NetDev => NETDEV_PER_ENDPOINT_MAPS,
            EndpointKind::Overlay => OVERLAY_PER_ENDPOINT_MAPS,
            EndpointKind::Wireguard => WIREGUARD_PER_ENDPOINT_MAPS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_str_and_from_kind_str_round_trip() {
        for kind in [
            EndpointKind::Container,
            EndpointKind::Host,
            EndpointKind::NetDev,
            EndpointKind::Overlay,
            EndpointKind::Wireguard,
        ] {
            assert_eq!(EndpointKind::from_kind_str(kind.kind_str()), Some(kind));
        }
        assert_eq!(EndpointKind::from_kind_str("bogus"), None);
    }
}
