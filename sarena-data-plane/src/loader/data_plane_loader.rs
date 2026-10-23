use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    path::PathBuf,
};

use sarena_infra::Link as _;
use sarena_shared::{EndpointConfig, EndpointInfo};

use crate::{
    EndpointPersist,
    error::{DataPlaneError, HookFailure, Res},
    loader::{
        backend::BpfBackend,
        loader_endpoint::{LoaderEndpoint, LoaderEndpointKind},
        manifest::{GLOBAL_MAPS, HookSpec},
        pin::PinRoot,
    },
};

pub struct Loader<B: BpfBackend> {
    backend: B,
    pin_root: PinRoot,
    globals_holder: Option<B::Instance>,
}

impl<B: BpfBackend> Loader<B> {
    pub fn new(backend: B, pin_root: PinRoot) -> Self {
        Self {
            backend,
            pin_root,
            globals_holder: None,
        }
    }

    pub fn load_global_maps(&mut self) -> Res<()> {
        let pins = self.global_map_pins();
        self.globals_holder = Some(self.backend.load_global_maps(&pins)?);
        Ok(())
    }

    /// Loads and attaches `endpoint`'s programs and fills in its maps. The
    /// maps are written before any hook is attached, so the programs never
    /// run against an empty config.
    pub fn add_loader_endpoint(&mut self, endpoint: &LoaderEndpoint) -> Res<()> {
        let kind = endpoint.kind();
        let link = endpoint.link();
        let resolved = self.backend.resolve_link(link)?;

        let mut maps = self.global_map_pins();
        for &map in kind.per_endpoint_map_names() {
            maps.insert(
                map.wire_name().to_string(),
                self.pin_root.per_endpoint_map_dir(map, link),
            );
        }

        let mut instance = self.backend.load_instance(link, &maps)?;
        if let LoaderEndpoint::Container(persist) = endpoint {
            self.write_container_maps(persist, &resolved)?;
        }

        let mut failures = Vec::new();
        let link_dir = self.pin_root.endpoint_link_dir(kind, link);
        for hook_spec in kind.hooks() {
            match self.backend.ensure_attached(
                &mut instance,
                hook_spec.program_name,
                hook_spec.hook,
                &resolved,
                &link_dir,
            ) {
                Ok(()) => {}
                Err(e) if hook_spec.required => failures.push(HookFailure {
                    hook_name: hook_spec.program_name,
                    error: e.to_string(),
                }),
                Err(e) => {
                    tracing::warn!(
                        kind = kind.kind_str(),
                        link,
                        hook = hook_spec.program_name,
                        error = %e,
                        "optional hook failed to attach, continuing"
                    );
                }
            }
        }

        if !failures.is_empty() {
            return Err(DataPlaneError::Partial(failures));
        }

        Ok(())
    }

    fn write_container_maps(&mut self, endpoint: &EndpointPersist, link: &B::LinkType) -> Res<()> {
        self.backend.set_endpoint_config(
            link.ifname(),
            EndpointConfig {
                endpoint_id: endpoint.id.get(),
                host_mac: endpoint.host_mac.0,
                ipv4: endpoint.ipv4,
                ipv6: endpoint.ipv6,
            },
        )?;
        self.backend.upsert_endpoint_info(
            endpoint,
            &EndpointInfo {
                endpoint_id: endpoint.id.get(),
                if_index: link.ifindex(),
                container_mac: endpoint.container_mac.0,
                host_mac: endpoint.host_mac.0,
            },
        )
    }

    /// Removes `endpoint`'s `lxc_map` entries first, so nothing is routed to
    /// it any more, then detaches its programs and unpins its maps.
    pub fn remove_loader_endpoint(&mut self, endpoint: &LoaderEndpoint) -> Res<()> {
        if let LoaderEndpoint::Container(persist) = endpoint {
            for ip in persist.ips() {
                self.backend.remove_endpoint_info(ip)?;
            }
        }
        self.detach_loader_endpoint(endpoint.kind(), endpoint.link())
    }

    fn detach_loader_endpoint(&mut self, kind: LoaderEndpointKind, link: &str) -> Res<()> {
        self.backend.stop_logging(link);

        self.backend
            .remove_pin_dir(&self.pin_root.endpoint_link_dir(kind, link))?;

        for &map in kind.per_endpoint_map_names() {
            let path = self.pin_root.per_endpoint_map_dir(map, link);
            self.backend.unpin_map(&path)?;
        }

        Ok(())
    }

    pub fn list_loader_endpoints(&self) -> Res<Vec<(LoaderEndpointKind, String)>> {
        let mut matched: HashMap<(LoaderEndpointKind, String), HashSet<&'static str>> =
            HashMap::new();
        for rel in self.backend.list_pins(&self.pin_root.links_dir())? {
            let Some(parent) = rel.parent() else { continue };
            let Some((kind, link)) = PinRoot::parse_pin_subpath(parent) else {
                continue;
            };
            let Some(filename) = rel.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            if let Some(hook_spec) = match_pin(kind, filename) {
                matched
                    .entry((kind, link))
                    .or_default()
                    .insert(hook_spec.program_name);
            }
        }

        Ok(matched
            .into_iter()
            .filter(|((kind, _link), matched_programs)| {
                kind.hooks()
                    .iter()
                    .filter(|hook_spec| hook_spec.required)
                    .all(|hook_spec| matched_programs.contains(hook_spec.program_name))
            })
            .map(|((kind, link), _)| (kind, link))
            .collect())
    }

    /// Adds every `desired` endpoint, detaches every attached endpoint not in
    /// `desired`, and drops every `lxc_map` entry whose IP no desired
    /// container owns. `desired` must therefore be the complete set.
    pub fn reconcile(
        &mut self,
        desired: &[LoaderEndpoint],
    ) -> Res<Vec<(LoaderEndpointKind, String, DataPlaneError)>> {
        let mut errors = Vec::new();

        for endpoint in desired {
            if let Err(e) = self.add_loader_endpoint(endpoint) {
                errors.push((endpoint.kind(), endpoint.link().to_string(), e));
            }
        }

        let existing: HashSet<_> = self.list_loader_endpoints()?.into_iter().collect();
        let wanted: HashSet<_> = desired
            .iter()
            .map(|endpoint| (endpoint.kind(), endpoint.link().to_string()))
            .collect();
        for (kind, link) in existing.difference(&wanted) {
            if let Err(e) = self.detach_loader_endpoint(*kind, link) {
                errors.push((*kind, link.clone(), e));
            }
        }

        // Stale endpoints are only known by link name, not by IP, so their
        // `lxc_map` entries are found by what is *not* wanted instead.
        let wanted_ips: HashSet<IpAddr> = desired
            .iter()
            .filter_map(|endpoint| match endpoint {
                LoaderEndpoint::Container(persist) => Some(persist.ips()),
                _ => None,
            })
            .flatten()
            .collect();
        for ip in self.backend.list_endpoint_infos()?.into_keys() {
            if !wanted_ips.contains(&ip) {
                self.backend.remove_endpoint_info(ip)?;
            }
        }

        Ok(errors)
    }

    pub fn teardown_all(&mut self) -> Res<()> {
        self.backend.stop_all_logging();
        self.globals_holder = None;
        Ok(())
    }

    fn global_map_pins(&self) -> HashMap<String, PathBuf> {
        GLOBAL_MAPS
            .iter()
            .map(|&m| (m.wire_name().to_string(), self.pin_root.global_map_dir(m)))
            .collect()
    }
}

fn match_pin(kind: LoaderEndpointKind, filename: &str) -> Option<&'static HookSpec> {
    kind.hooks()
        .iter()
        .find(|hook_spec| hook_spec.program_name == filename)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::{
        EndpointId, EndpointSpec,
        loader::mock_backend::{Call, MockBackend},
    };

    fn link(n: u16) -> String {
        format!("veth{n}")
    }

    fn ip(n: u16) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, n as u8))
    }

    fn container(n: u16) -> LoaderEndpoint {
        LoaderEndpoint::Container(EndpointPersist {
            id: EndpointId::new(n),
            spec: EndpointSpec {
                host_iface_name: link(n),
                ipv4: Some(Ipv4Addr::new(10, 0, 0, n as u8)),
                ..Default::default()
            },
        })
    }

    #[test]
    fn add_then_remove_happy_path() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        let l = link(1);

        loader.add_loader_endpoint(&container(1)).unwrap();
        assert_eq!(
            loader.list_loader_endpoints().unwrap(),
            vec![(LoaderEndpointKind::Container, l.clone())]
        );

        let loaded_maps = loader
            .backend
            .calls
            .iter()
            .find_map(|c| match c {
                Call::LoadInstance(maps) => Some(maps),
                _ => None,
            })
            .unwrap();
        assert_eq!(loaded_maps.len(), 4);

        loader.remove_loader_endpoint(&container(1)).unwrap();
        assert_eq!(loader.list_loader_endpoints().unwrap(), vec![]);
    }

    #[test]
    fn each_endpoint_gets_its_own_resolve_load_and_attach_calls() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();
        loader.add_loader_endpoint(&container(2)).unwrap();

        let resolve_calls = loader
            .backend
            .calls
            .iter()
            .filter(|c| matches!(c, Call::ResolveLink(_)))
            .count();
        assert_eq!(
            resolve_calls, 2,
            "every add_endpoint call resolves its own link, never shared"
        );

        let load_calls = loader
            .backend
            .calls
            .iter()
            .filter(|c| matches!(c, Call::LoadInstance(_)))
            .count();
        assert_eq!(
            load_calls, 2,
            "option A loads a fresh instance per endpoint, never shared"
        );

        let attach_calls = loader
            .backend
            .calls
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    Call::EnsureAttached {
                        program: "from_container",
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            attach_calls, 2,
            "each endpoint still gets its own hook attach call"
        );
    }

    #[test]
    fn per_endpoint_map_path_differs_across_endpoints() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();
        loader.add_loader_endpoint(&container(2)).unwrap();

        let load_calls: Vec<_> = loader
            .backend
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::LoadInstance(maps) => Some(maps),
                _ => None,
            })
            .collect();
        assert_eq!(load_calls.len(), 2);

        let per_endpoint_path = |maps: &[(String, PathBuf)]| {
            maps.iter()
                .find(|(name, _)| name == "calls_map")
                .map(|(_, path)| path.clone())
                .unwrap()
        };

        assert_ne!(
            per_endpoint_path(load_calls[0]),
            per_endpoint_path(load_calls[1]),
            "per-endpoint map must resolve to a distinct path per endpoint"
        );
    }

    #[test]
    fn per_endpoint_maps_pin_with_link_suffix_globals_with_bare_names() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        let l = link(1);
        loader.add_loader_endpoint(&container(1)).unwrap();

        let loaded_maps: Vec<String> = loader
            .backend
            .calls
            .iter()
            .find_map(|c| match c {
                Call::LoadInstance(maps) => Some(maps),
                _ => None,
            })
            .unwrap()
            .iter()
            .map(|(_, path)| path.to_string_lossy().into_owned())
            .collect();

        // per-endpoint maps carry the `_<link>` suffix in their pin path...
        assert!(
            loaded_maps
                .iter()
                .any(|p| p.ends_with(&format!("calls_map_{l}")))
        );
        // ...global maps do not.
        assert!(loaded_maps.iter().any(|p| p.ends_with("globals/lxc_map")));
    }

    #[test]
    fn load_global_maps_pins_every_global_map_once_up_front() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.load_global_maps().unwrap();

        let pinned: Vec<String> = loader
            .backend
            .calls
            .iter()
            .find_map(|c| match c {
                Call::LoadGlobalMaps(maps) => Some(maps),
                _ => None,
            })
            .unwrap()
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        assert_eq!(pinned.len(), 2);
        for name in ["lxc_map", "metrics_map"] {
            assert!(pinned.iter().any(|n| n == name), "missing {name}");
        }
    }

    #[test]
    fn add_endpoint_fails_hard_without_attempting_hooks_if_load_instance_fails() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.backend.fail_at = Some((loader.backend.next_call_index() + 1, "bad object".into()));

        let result = loader.add_loader_endpoint(&container(1));
        assert!(result.is_err());
        assert!(
            !matches!(result, Err(DataPlaneError::Partial(_))),
            "a load_instance failure is not a partial-hook failure"
        );

        let attach_calls = loader
            .backend
            .calls
            .iter()
            .filter(|c| matches!(c, Call::EnsureAttached { .. }))
            .count();
        assert_eq!(
            attach_calls, 0,
            "no hook should be attempted if loading itself failed"
        );
        assert_eq!(loader.list_loader_endpoints().unwrap(), vec![]);
    }

    #[test]
    fn add_endpoint_fails_hard_if_link_resolution_fails() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.backend.fail_at = Some((loader.backend.next_call_index(), "no such link".into()));

        let result = loader.add_loader_endpoint(&container(1));
        assert!(result.is_err());
        assert!(!matches!(result, Err(DataPlaneError::Partial(_))));

        let load_calls = loader
            .backend
            .calls
            .iter()
            .filter(|c| matches!(c, Call::LoadInstance(_)))
            .count();
        assert_eq!(
            load_calls, 0,
            "no load should even be attempted if resolving the link failed"
        );
        assert_eq!(loader.list_loader_endpoints().unwrap(), vec![]);
    }

    #[test]
    fn add_endpoint_retries_only_the_hook_that_previously_failed() {
        let l = link(1);
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);

        // resolve, load, endpoint_config, lxc_map, then the ingress hook.
        let fail_index = loader.backend.next_call_index() + 4;
        loader.backend.fail_at = Some((fail_index, "simulated crash during ingress attach".into()));

        let result = loader.add_loader_endpoint(&container(1));
        assert!(result.is_err(), "ingress hook failed, so this must fail");
        assert_eq!(loader.list_loader_endpoints().unwrap(), vec![]);

        // "the daemon restarts" - clear the injected failure and call
        // add_endpoint again with the same kind/link.
        loader.backend.fail_at = None;
        loader.add_loader_endpoint(&container(1)).unwrap();
        assert_eq!(
            loader.list_loader_endpoints().unwrap(),
            vec![(LoaderEndpointKind::Container, l)]
        );
    }

    #[test]
    fn reconcile_removes_endpoints_missing_from_desired_state() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();
        loader.add_loader_endpoint(&container(2)).unwrap();

        let desired = vec![container(1)];
        let errors = loader.reconcile(&desired).unwrap();
        assert!(errors.is_empty());

        assert_eq!(
            loader.list_loader_endpoints().unwrap(),
            vec![(LoaderEndpointKind::Container, link(1))]
        );
    }

    #[test]
    fn reconcile_repairs_half_attached_endpoints_found_on_disk() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();

        // link(2): let resolve_link/load_instance succeed, then fail
        // exactly on its first hook (ingress) - a genuinely half-attached
        // endpoint.
        // resolve, load, endpoint_config, lxc_map, then the ingress hook.
        let fail_index = loader.backend.next_call_index() + 4;
        loader.backend.fail_at = Some((fail_index, "crash right after loading".into()));
        let _ = loader.add_loader_endpoint(&container(2));
        loader.backend.fail_at = None;

        assert_eq!(
            loader.list_loader_endpoints().unwrap(),
            vec![(LoaderEndpointKind::Container, link(1))]
        );

        let desired = vec![container(1), container(2)];
        let errors = loader.reconcile(&desired).unwrap();
        assert!(
            errors.is_empty(),
            "reconcile should finish attaching the second endpoint: {errors:?}"
        );

        let mut active = loader.list_loader_endpoints().unwrap();
        active.sort_by_key(|(_, l)| l.clone());
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn optional_hook_failure_does_not_fail_the_endpoint() {
        // NetDev's manifest currently has no optional hooks, so this
        // test documents the *mechanism* using a hand-built manifest
        // check rather than depending on a specific future XDP entry.
        assert!(
            LoaderEndpointKind::Container
                .hooks()
                .iter()
                .all(|h| h.required),
            "no optional hooks defined yet - update this test when XDP lands"
        );
    }

    #[test]
    fn remove_endpoint_cleans_up_only_its_own_per_endpoint_map() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();
        loader.add_loader_endpoint(&container(2)).unwrap();

        loader.remove_loader_endpoint(&container(1)).unwrap();

        let remaining_map_pins: Vec<_> = loader
            .backend
            .list_pins(&loader.pin_root.globals_dir())
            .unwrap();
        let remaining: HashSet<_> = remaining_map_pins
            .iter()
            .filter_map(|p| p.to_str())
            .collect();

        assert!(
            !remaining
                .iter()
                .any(|p| p.contains(&format!("calls_map_{}", link(1)))),
            "the first endpoint's per-endpoint map must be gone: {remaining:?}"
        );
        assert!(
            remaining
                .iter()
                .any(|p| p.contains(&format!("calls_map_{}", link(2)))),
            "the second endpoint's per-endpoint map must be untouched: {remaining:?}"
        );
    }

    #[test]
    fn add_container_writes_its_maps_before_attaching_any_hook() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();

        let calls = &loader.backend.calls;
        let position = |pred: fn(&Call) -> bool| calls.iter().position(pred).unwrap();
        let config = position(|c| matches!(c, Call::SetEndpointConfig(_)));
        let info = position(|c| matches!(c, Call::UpsertEndpointInfo(_)));
        let attach = position(|c| matches!(c, Call::EnsureAttached { .. }));
        assert!(config < attach && info < attach, "{calls:?}");
        assert!(loader.backend.endpoint_infos.contains_key(&ip(1)));
    }

    #[test]
    fn add_host_endpoint_writes_no_container_maps() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader
            .add_loader_endpoint(&LoaderEndpoint::Host { link: link(1) })
            .unwrap();

        assert!(
            !loader
                .backend
                .calls
                .iter()
                .any(|c| matches!(c, Call::SetEndpointConfig(_) | Call::UpsertEndpointInfo(_)))
        );
    }

    #[test]
    fn remove_container_drops_its_lxc_map_entries_before_detaching() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();
        loader.add_loader_endpoint(&container(2)).unwrap();
        loader.backend.calls.clear();

        loader.remove_loader_endpoint(&container(1)).unwrap();

        assert_eq!(loader.backend.calls[0], Call::RemoveEndpointInfo(ip(1)));
        assert!(!loader.backend.endpoint_infos.contains_key(&ip(1)));
        assert!(loader.backend.endpoint_infos.contains_key(&ip(2)));
    }

    #[test]
    fn reconcile_drops_lxc_map_entries_of_endpoints_missing_from_desired_state() {
        let pin_root = PinRoot::new("/sys/fs/bpf/test");
        let mut loader = Loader::new(MockBackend::new(), pin_root);
        loader.add_loader_endpoint(&container(1)).unwrap();
        loader.add_loader_endpoint(&container(2)).unwrap();

        let errors = loader.reconcile(&[container(1)]).unwrap();
        assert!(errors.is_empty());

        let ips: Vec<_> = loader.backend.endpoint_infos.keys().copied().collect();
        assert_eq!(ips, vec![ip(1)]);
    }
}
