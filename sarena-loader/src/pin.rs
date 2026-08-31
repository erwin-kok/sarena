use std::path::{Path, PathBuf};

use crate::{
    endpoint::EndpointKind,
    maps::{EndpointMap, GlobalMap},
};

/// Resolves every pin path this loader ever touches, all under one
/// root (e.g. `/sys/fs/bpf/sarena`). Pure and deterministic on
/// purpose - no I/O here, so every method is trivially unit-testable
/// and there's exactly one place that knows the on-disk layout.
///
/// Layout (uniform across every kind - `link` is always the network
/// interface name):
/// ```text
/// <root>/links/<kind>/<link>/<prog_name>
///
/// <root>/globals/<map_name>        global maps
/// <root>/globals/<map_name>_<link> per-endpoint maps
/// ```
#[derive(Clone, Debug)]
pub struct PinRoot(PathBuf);

impl PinRoot {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(root.into())
    }

    #[allow(dead_code)]
    pub fn root(&self) -> &Path {
        &self.0
    }

    pub fn links_dir(&self) -> PathBuf {
        self.0.join("links")
    }

    pub fn globals_dir(&self) -> PathBuf {
        self.0.join("globals")
    }

    pub fn endpoint_link_dir(&self, kind: EndpointKind, link: &str) -> PathBuf {
        self.links_dir().join(pin_subpath(kind, link))
    }

    pub fn per_endpoint_map_dir(&self, map: EndpointMap, link: &str) -> PathBuf {
        self.globals_dir()
            .join(format!("{}_{link}", map.wire_name()))
    }

    pub fn global_map_dir(&self, map: GlobalMap) -> PathBuf {
        self.globals_dir().join(map.wire_name())
    }

    pub fn parse_pin_subpath(path: &Path) -> Option<(EndpointKind, String)> {
        let mut components = path.components();
        let kind = EndpointKind::from_kind_str(components.next()?.as_os_str().to_str()?)?;
        let link = components.next()?.as_os_str().to_str()?.to_string();
        Some((kind, link))
    }
}

fn pin_subpath(kind: EndpointKind, link: &str) -> PathBuf {
    Path::new(kind.kind_str()).join(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_subpath_roundtrips_for_every_kind() {
        let kinds = [
            EndpointKind::Container,
            EndpointKind::Host,
            EndpointKind::NetDev,
            EndpointKind::Overlay,
            EndpointKind::Wireguard,
        ];
        for kind in kinds {
            let sub = pin_subpath(kind, "eth0");
            let (parsed_kind, parsed_link) = PinRoot::parse_pin_subpath(&sub)
                .unwrap_or_else(|| panic!("failed to parse {sub:?} back into a kind/link pair"));
            assert_eq!(kind, parsed_kind);
            assert_eq!(parsed_link, "eth0");
        }
    }

    #[test]
    fn garbage_pin_paths_are_rejected_not_guessed() {
        assert_eq!(PinRoot::parse_pin_subpath(Path::new("bogus/123")), None);
        assert_eq!(PinRoot::parse_pin_subpath(Path::new("")), None);
    }

    #[test]
    fn endpoint_link_dirs_are_scoped_per_endpoint() {
        let root = PinRoot::new("/sys/fs/bpf/test");

        assert_ne!(
            root.endpoint_link_dir(EndpointKind::Container, "lxc00001"),
            root.endpoint_link_dir(EndpointKind::Container, "lxc00002")
        );
        assert_ne!(
            root.endpoint_link_dir(EndpointKind::Container, "eth0"),
            root.endpoint_link_dir(EndpointKind::Host, "eth0"),
            "the same link name under a different kind must not collide"
        );
    }

    #[test]
    fn per_endpoint_map_paths_do_not_collide() {
        let root = PinRoot::new("/sys/fs/bpf/test");
        assert_ne!(
            root.per_endpoint_map_dir(EndpointMap::CallsMap, "lxc00001"),
            root.per_endpoint_map_dir(EndpointMap::CallsMap, "lxc00002")
        );
    }

    #[test]
    fn global_map_path_does_not_collide_with_per_endpoint_paths() {
        let root = PinRoot::new("/sys/fs/bpf/test");
        // A per-endpoint map's pin carries a `_<link>` suffix, so it can
        // never alias a bare global-map pin even for the same wire name.
        assert_ne!(
            root.global_map_dir(GlobalMap::LxcMap),
            root.per_endpoint_map_dir(EndpointMap::CallsMap, "lxc00001")
        );
        assert!(
            root.per_endpoint_map_dir(EndpointMap::CallsMap, "lxc00001")
                .to_string_lossy()
                .contains("calls_map_lxc00001")
        );
    }
}
