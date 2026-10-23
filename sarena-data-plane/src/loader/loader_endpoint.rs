use crate::{
    EndpointPersist,
    loader::manifest::{
        CONTAINER_HOOKS, CONTAINER_PER_ENDPOINT_MAPS, HOST_HOOKS, HOST_PER_ENDPOINT_MAPS, HookSpec,
        NETDEV_HOOKS, NETDEV_PER_ENDPOINT_MAPS, OVERLAY_HOOKS, OVERLAY_PER_ENDPOINT_MAPS,
        WIREGUARD_HOOKS, WIREGUARD_PER_ENDPOINT_MAPS,
    },
    maps::EndpointMap,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoaderEndpointKind {
    Container,
    Host,
    NetDev,
    Overlay,
    Wireguard,
}

impl LoaderEndpointKind {
    pub fn kind_str(self) -> &'static str {
        match self {
            LoaderEndpointKind::Container => "container",
            LoaderEndpointKind::Host => "host",
            LoaderEndpointKind::NetDev => "netdev",
            LoaderEndpointKind::Overlay => "overlay",
            LoaderEndpointKind::Wireguard => "wireguard",
        }
    }

    pub fn from_kind_str(s: &str) -> Option<Self> {
        Some(match s {
            "container" => LoaderEndpointKind::Container,
            "host" => LoaderEndpointKind::Host,
            "netdev" => LoaderEndpointKind::NetDev,
            "overlay" => LoaderEndpointKind::Overlay,
            "wireguard" => LoaderEndpointKind::Wireguard,
            _ => return None,
        })
    }

    pub(crate) fn hooks(self) -> &'static [HookSpec] {
        match self {
            LoaderEndpointKind::Container => CONTAINER_HOOKS,
            LoaderEndpointKind::Host => HOST_HOOKS,
            LoaderEndpointKind::NetDev => NETDEV_HOOKS,
            LoaderEndpointKind::Overlay => OVERLAY_HOOKS,
            LoaderEndpointKind::Wireguard => WIREGUARD_HOOKS,
        }
    }

    pub(crate) fn per_endpoint_map_names(self) -> &'static [EndpointMap] {
        match self {
            LoaderEndpointKind::Container => CONTAINER_PER_ENDPOINT_MAPS,
            LoaderEndpointKind::Host => HOST_PER_ENDPOINT_MAPS,
            LoaderEndpointKind::NetDev => NETDEV_PER_ENDPOINT_MAPS,
            LoaderEndpointKind::Overlay => OVERLAY_PER_ENDPOINT_MAPS,
            LoaderEndpointKind::Wireguard => WIREGUARD_PER_ENDPOINT_MAPS,
        }
    }
}

/// An endpoint the loader attaches programs to, together with whatever
/// that kind of endpoint needs to fill in its maps.
#[derive(Clone, Debug, PartialEq)]
pub enum LoaderEndpoint {
    /// A container's host-side veth. Gets an `endpoint_config` and an
    /// `lxc_map` entry per IP.
    Container(EndpointPersist),
    Host {
        link: String,
    },
    NetDev {
        link: String,
    },
    Overlay {
        link: String,
    },
    Wireguard {
        link: String,
    },
}

impl LoaderEndpoint {
    pub fn kind(&self) -> LoaderEndpointKind {
        match self {
            LoaderEndpoint::Container(_) => LoaderEndpointKind::Container,
            LoaderEndpoint::Host { .. } => LoaderEndpointKind::Host,
            LoaderEndpoint::NetDev { .. } => LoaderEndpointKind::NetDev,
            LoaderEndpoint::Overlay { .. } => LoaderEndpointKind::Overlay,
            LoaderEndpoint::Wireguard { .. } => LoaderEndpointKind::Wireguard,
        }
    }

    pub fn link(&self) -> &str {
        match self {
            LoaderEndpoint::Container(endpoint) => &endpoint.host_iface_name,
            LoaderEndpoint::Host { link }
            | LoaderEndpoint::NetDev { link }
            | LoaderEndpoint::Overlay { link }
            | LoaderEndpoint::Wireguard { link } => link,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_str_and_from_kind_str_round_trip() {
        for kind in [
            LoaderEndpointKind::Container,
            LoaderEndpointKind::Host,
            LoaderEndpointKind::NetDev,
            LoaderEndpointKind::Overlay,
            LoaderEndpointKind::Wireguard,
        ] {
            assert_eq!(
                LoaderEndpointKind::from_kind_str(kind.kind_str()),
                Some(kind)
            );
        }
        assert_eq!(LoaderEndpointKind::from_kind_str("bogus"), None);
    }
}
