use std::path::{Path, PathBuf};

use crate::{ContainerId, endpoint::EndpointId};

/// Resolves every pin path this loader ever touches, all under one
/// root (e.g. `/sys/fs/bpf/sarena`). Pure and deterministic on
/// purpose - no I/O here, so every method is trivially unit-testable
/// and there's exactly one place that knows the on-disk layout.
///
/// Layout:
/// ```text
/// <root>/links/container/<container_id>/<prog_name>
/// <root>/links/host/<link_name>/<prog_name>
/// <root>/links/netdev/<link_name>/<prog_name>
/// <root>/links/overlay/<link_name>/<prog_name>
///
/// <root>/globals/<map_name>                   global maps
/// <root>/globals/<map_name>_<endpoint_key>    per-endpoint maps
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

    pub fn endpoint_link_dir(&self, id: &EndpointId) -> PathBuf {
        self.links_dir().join(pin_subpath(id))
    }

    pub fn per_endpoint_map_dir(&self, map_name: &str, id: &EndpointId) -> PathBuf {
        self.globals_dir()
            .join(format!("{map_name}_{}", endpoint_key(id)))
    }

    pub fn parse_pin_subpath(path: &Path) -> Option<EndpointId> {
        let mut components = path.components();
        let kind = components.next()?.as_os_str().to_str()?;
        match kind {
            "container" => {
                let raw = components.next()?.as_os_str().to_str()?;
                let id: u16 = raw.parse().ok()?;
                Some(EndpointId::Container(ContainerId(id)))
            }
            "host" => {
                let name = components.next()?.as_os_str().to_str()?;
                Some(EndpointId::Host(name.to_string()))
            }
            "netdev" => {
                let name = components.next()?.as_os_str().to_str()?;
                Some(EndpointId::NetDev(name.to_string()))
            }
            "overlay" => {
                let name = components.next()?.as_os_str().to_str()?;
                Some(EndpointId::Overlay(name.to_string()))
            }
            "wireguard" => {
                let name = components.next()?.as_os_str().to_str()?;
                Some(EndpointId::Wireguard(name.to_string()))
            }
            _ => None,
        }
    }
}

fn endpoint_key(id: &EndpointId) -> String {
    match id {
        EndpointId::Container(cid) => cid.to_string(),
        EndpointId::Host(link) => link.clone(),
        EndpointId::NetDev(link) => link.clone(),
        EndpointId::Overlay(link) => link.clone(),
        EndpointId::Wireguard(link) => link.clone(),
    }
}

fn pin_subpath(id: &EndpointId) -> PathBuf {
    match id {
        EndpointId::Container(cid) => Path::new("container").join(cid.to_string()),
        EndpointId::Host(link) => Path::new("host").join(link),
        EndpointId::NetDev(link) => Path::new("netdev").join(link),
        EndpointId::Overlay(link) => Path::new("overlay").join(link),
        EndpointId::Wireguard(link) => Path::new("wireguard").join(link),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::ContainerId;

    #[test]
    fn pin_subpath_roundtrips_for_every_kind() {
        let ids = [
            EndpointId::Container(ContainerId(42)),
            EndpointId::Host("host".to_string()),
            EndpointId::NetDev("eth0".to_string()),
            EndpointId::Overlay("overlay".to_string()),
        ];
        for id in ids {
            let sub = pin_subpath(&id);
            let parsed = PinRoot::parse_pin_subpath(&sub)
                .unwrap_or_else(|| panic!("failed to parse {sub:?} back into an EndpointId"));
            assert_eq!(id, parsed);
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
        let a = EndpointId::Container(ContainerId(1));
        let b = EndpointId::Container(ContainerId(2));

        assert_ne!(root.endpoint_link_dir(&a), root.endpoint_link_dir(&b));
    }

    #[test]
    fn per_endpoint_map_paths_do_not_collide() {
        let root = PinRoot::new("/sys/fs/bpf/test");
        let a = EndpointId::Container(ContainerId(1));
        let b = EndpointId::Container(ContainerId(2));
        assert_ne!(
            root.per_endpoint_map_dir("sarena_policy", &a),
            root.per_endpoint_map_dir("sarena_policy", &b)
        );
    }
}
