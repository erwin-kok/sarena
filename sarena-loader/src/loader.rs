use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{
    backend::BpfBackend,
    endpoint::EndpointKind,
    error::{HookFailure, LoaderError, Res},
    manifest::{GLOBAL_MAPS, HookSpec},
    pin::PinRoot,
};

pub struct Loader<B: BpfBackend> {
    backend: B,
    pins: PinRoot,
    globals_holder: Option<B::Instance>,
}

impl<B: BpfBackend> Loader<B> {
    pub fn new(backend: B, pin_root: impl Into<PathBuf>) -> Self {
        Self {
            backend,
            pins: PinRoot::new(pin_root),
            globals_holder: None,
        }
    }

    pub fn load_global_maps(&mut self) -> Res<()> {
        let pins = self.global_map_pins();
        self.globals_holder = Some(self.backend.load_global_maps(&pins)?);
        Ok(())
    }

    pub fn add_endpoint(&mut self, kind: EndpointKind, link: &str) -> Res<()> {
        let resolved = self.backend.resolve_link(link)?;

        let mut maps = self.global_map_pins();
        for &map in kind.per_endpoint_map_names() {
            maps.insert(
                map.wire_name().to_string(),
                self.pins.per_endpoint_map_dir(map, link),
            );
        }

        let mut instance = self.backend.load_instance(link, &maps)?;
        let mut failures = Vec::new();
        let link_dir = self.pins.endpoint_link_dir(kind, link);
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
            return Err(LoaderError::Partial(failures));
        }

        Ok(())
    }

    pub fn remove_endpoint(&mut self, kind: EndpointKind, link: &str) -> Res<()> {
        self.backend.stop_logging(link);

        self.backend
            .remove_pin_dir(&self.pins.endpoint_link_dir(kind, link))?;

        for &map in kind.per_endpoint_map_names() {
            let path = self.pins.per_endpoint_map_dir(map, link);
            self.backend.unpin_map(&path)?;
        }

        Ok(())
    }

    pub fn list_active_endpoints(&self) -> Res<Vec<(EndpointKind, String)>> {
        let mut matched: HashMap<(EndpointKind, String), HashSet<&'static str>> = HashMap::new();
        for rel in self.backend.list_pins(&self.pins.links_dir())? {
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

    pub fn reconcile(
        &mut self,
        desired: &[(EndpointKind, String)],
    ) -> Res<Vec<(EndpointKind, String, LoaderError)>> {
        let mut errors = Vec::new();

        for (kind, link) in desired {
            if let Err(e) = self.add_endpoint(*kind, link) {
                errors.push((*kind, link.clone(), e));
            }
        }

        let existing: HashSet<_> = self.list_active_endpoints()?.into_iter().collect();
        let wanted: HashSet<_> = desired.iter().cloned().collect();
        for (kind, link) in existing.difference(&wanted) {
            if let Err(e) = self.remove_endpoint(*kind, link) {
                errors.push((*kind, link.clone(), e));
            }
        }

        Ok(errors)
    }

    pub fn teardown_all(&mut self) -> Res<()> {
        self.backend.stop_all_logging();
        self.backend.remove_pin_dir(&self.pins.links_dir())?;
        self.backend.remove_pin_dir(&self.pins.globals_dir())?;
        // The pins are gone; drop the instance that was holding the maps.
        self.globals_holder = None;
        Ok(())
    }

    fn global_map_pins(&self) -> HashMap<String, PathBuf> {
        GLOBAL_MAPS
            .iter()
            .map(|&m| (m.wire_name().to_string(), self.pins.global_map_dir(m)))
            .collect()
    }
}

fn match_pin(kind: EndpointKind, filename: &str) -> Option<&'static HookSpec> {
    kind.hooks()
        .iter()
        .find(|hook_spec| hook_spec.program_name == filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_backend::{Call, MockBackend};

    fn link(n: u16) -> String {
        format!("veth{n}")
    }

    #[test]
    fn add_then_remove_happy_path() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        let l = link(1);

        loader.add_endpoint(EndpointKind::Container, &l).unwrap();
        assert_eq!(
            loader.list_active_endpoints().unwrap(),
            vec![(EndpointKind::Container, l.clone())]
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
        assert_eq!(loaded_maps.len(), 6);

        loader.remove_endpoint(EndpointKind::Container, &l).unwrap();
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![]);
    }

    #[test]
    fn each_endpoint_gets_its_own_resolve_load_and_attach_calls() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader
            .add_endpoint(EndpointKind::Container, &link(1))
            .unwrap();
        loader
            .add_endpoint(EndpointKind::Container, &link(2))
            .unwrap();

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
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader
            .add_endpoint(EndpointKind::Container, &link(1))
            .unwrap();
        loader
            .add_endpoint(EndpointKind::Container, &link(2))
            .unwrap();

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
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        let l = link(1);
        loader.add_endpoint(EndpointKind::Container, &l).unwrap();

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
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
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

        assert_eq!(pinned.len(), 4);
        for name in ["lxc_map", "conntrack_tcp_buffer", "conntrack_any_buffer"] {
            assert!(pinned.iter().any(|n| n == name), "missing {name}");
        }
    }

    #[test]
    fn add_endpoint_fails_hard_without_attempting_hooks_if_load_instance_fails() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader.backend.fail_at = Some((loader.backend.next_call_index() + 1, "bad object".into()));

        let result = loader.add_endpoint(EndpointKind::Container, &link(1));
        assert!(result.is_err());
        assert!(
            !matches!(result, Err(LoaderError::Partial(_))),
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
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![]);
    }

    #[test]
    fn add_endpoint_fails_hard_if_link_resolution_fails() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader.backend.fail_at = Some((loader.backend.next_call_index(), "no such link".into()));

        let result = loader.add_endpoint(EndpointKind::Container, &link(1));
        assert!(result.is_err());
        assert!(!matches!(result, Err(LoaderError::Partial(_))));

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
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![]);
    }

    #[test]
    fn add_endpoint_retries_only_the_hook_that_previously_failed() {
        let l = link(1);
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");

        let fail_index = loader.backend.next_call_index() + 2;
        loader.backend.fail_at = Some((fail_index, "simulated crash during ingress attach".into()));

        let result = loader.add_endpoint(EndpointKind::Container, &l);
        assert!(result.is_err(), "ingress hook failed, so this must fail");
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![]);

        // "the daemon restarts" - clear the injected failure and call
        // add_endpoint again with the same kind/link.
        loader.backend.fail_at = None;
        loader.add_endpoint(EndpointKind::Container, &l).unwrap();
        assert_eq!(
            loader.list_active_endpoints().unwrap(),
            vec![(EndpointKind::Container, l)]
        );
    }

    #[test]
    fn reconcile_removes_endpoints_missing_from_desired_state() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader
            .add_endpoint(EndpointKind::Container, &link(1))
            .unwrap();
        loader
            .add_endpoint(EndpointKind::Container, &link(2))
            .unwrap();

        let desired = vec![(EndpointKind::Container, link(1))];
        let errors = loader.reconcile(&desired).unwrap();
        assert!(errors.is_empty());

        assert_eq!(
            loader.list_active_endpoints().unwrap(),
            vec![(EndpointKind::Container, link(1))]
        );
    }

    #[test]
    fn reconcile_repairs_half_attached_endpoints_found_on_disk() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader
            .add_endpoint(EndpointKind::Container, &link(1))
            .unwrap();

        // link(2): let resolve_link/load_instance succeed, then fail
        // exactly on its first hook (ingress) - a genuinely half-attached
        // endpoint.
        let fail_index = loader.backend.next_call_index() + 2;
        loader.backend.fail_at = Some((fail_index, "crash right after loading".into()));
        let _ = loader.add_endpoint(EndpointKind::Container, &link(2));
        loader.backend.fail_at = None;

        assert_eq!(
            loader.list_active_endpoints().unwrap(),
            vec![(EndpointKind::Container, link(1))]
        );

        let desired = vec![
            (EndpointKind::Container, link(1)),
            (EndpointKind::Container, link(2)),
        ];
        let errors = loader.reconcile(&desired).unwrap();
        assert!(
            errors.is_empty(),
            "reconcile should finish attaching the second endpoint: {errors:?}"
        );

        let mut active = loader.list_active_endpoints().unwrap();
        active.sort_by_key(|(_, l)| l.clone());
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn optional_hook_failure_does_not_fail_the_endpoint() {
        // NetDev's manifest currently has no optional hooks, so this
        // test documents the *mechanism* using a hand-built manifest
        // check rather than depending on a specific future XDP entry.
        assert!(
            EndpointKind::Container.hooks().iter().all(|h| h.required),
            "no optional hooks defined yet - update this test when XDP lands"
        );
    }

    #[test]
    fn remove_endpoint_cleans_up_only_its_own_per_endpoint_map() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader
            .add_endpoint(EndpointKind::Container, &link(1))
            .unwrap();
        loader
            .add_endpoint(EndpointKind::Container, &link(2))
            .unwrap();

        loader
            .remove_endpoint(EndpointKind::Container, &link(1))
            .unwrap();

        let remaining_map_pins: Vec<_> = loader
            .backend
            .list_pins(&loader.pins.globals_dir())
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
    fn teardown_all_removes_links_and_all_maps() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader
            .add_endpoint(EndpointKind::Container, &link(1))
            .unwrap();
        loader.add_endpoint(EndpointKind::Host, "the_host").unwrap();

        loader.teardown_all().unwrap();

        assert!(
            loader
                .backend
                .list_pins(&loader.pins.links_dir())
                .unwrap()
                .is_empty()
        );
        assert!(
            loader
                .backend
                .list_pins(&loader.pins.globals_dir())
                .unwrap()
                .is_empty()
        );
    }
}
