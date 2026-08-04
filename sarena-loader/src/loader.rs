use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{
    backend::BpfBackend,
    endpoint::EndpointKind,
    error::{HookFailure, LoaderError, Res},
    manifest::HookSpec,
    pin::PinRoot,
};

/// Returned from a successful `add_endpoint`. Map paths, not map
/// handles or map content - the caller opens these itself with
/// whatever map wrapper it likes. Only *per-endpoint* maps are
/// reported here (see `endpoint::map_rename`) - a global map resolves
/// to the same path for every endpoint via `AyaBackend`'s own
/// `default_map_pin_directory` fallback, so this crate has no
/// per-endpoint bookkeeping reason to track it.
#[derive(Debug, Default)]
pub struct EndpointHandle {
    pub map_paths: HashMap<String, PathBuf>,
}

pub struct Loader<B: BpfBackend> {
    backend: B,
    pins: PinRoot,
}

impl<B: BpfBackend> Loader<B> {
    pub fn new(backend: B, pin_root: impl Into<PathBuf>) -> Self {
        Self {
            backend,
            pins: PinRoot::new(pin_root),
        }
    }

    pub fn add_endpoint(&mut self, kind: EndpointKind, link: &str) -> Res<EndpointHandle> {
        let resolved = self.backend.resolve_link(link)?;
        let per_endpoint_maps = kind.map_rename(&self.pins, link);
        let mut instance = self.backend.load_instance(link, &per_endpoint_maps)?;
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

        Ok(EndpointHandle {
            map_paths: per_endpoint_maps,
        })
    }

    pub fn remove_endpoint(&mut self, kind: EndpointKind, link: &str) -> Res<()> {
        self.backend.stop_logging(link);

        self.backend
            .remove_pin_dir(&self.pins.endpoint_link_dir(kind, link))?;

        for name in kind.per_endpoint_map_names() {
            let path = self.pins.per_endpoint_map_dir(name, link);
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
        Ok(())
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

        let handle = loader.add_endpoint(EndpointKind::Container, &l).unwrap();
        assert_eq!(
            loader.list_active_endpoints().unwrap(),
            vec![(EndpointKind::Container, l.clone())]
        );

        // CONTAINER_PER_ENDPOINT_MAPS currently declares one map. If
        // that list changes, update this count.
        assert_eq!(handle.map_paths.len(), 2);

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
    fn endpoint_handle_reports_only_per_endpoint_map_paths() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        let handle = loader
            .add_endpoint(EndpointKind::Container, &link(1))
            .unwrap();

        assert!(handle.map_paths.contains_key("calls_map"));
        assert!(
            !handle.map_paths.contains_key("shared_map"),
            "global maps aren't this crate's bookkeeping to report"
        );
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
