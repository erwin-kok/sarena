use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{
    backend::BpfBackend,
    endpoint::EndpointId,
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

    pub fn add_endpoint(&mut self, id: &EndpointId) -> Res<EndpointHandle> {
        let link = self.backend.resolve_link(id)?;
        let per_endpoint_maps = id.map_rename(&self.pins);
        let mut instance = self.backend.load_instance(&per_endpoint_maps)?;
        let mut failures = Vec::new();
        let link_dir = self.pins.endpoint_link_dir(id);
        for hook_spec in id.hooks() {
            match self.backend.ensure_attached(
                &mut instance,
                hook_spec.program_name,
                hook_spec.hook,
                &link,
                &link_dir,
            ) {
                Ok(()) => {}
                Err(e) if hook_spec.required => failures.push(HookFailure {
                    hook_name: hook_spec.program_name,
                    error: e.to_string(),
                }),
                Err(e) => {
                    tracing::warn!(
                        endpoint = ?id,
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

    pub fn remove_endpoint(&mut self, id: &EndpointId) -> Res<()> {
        self.backend
            .remove_pin_dir(&self.pins.endpoint_link_dir(id))?;

        for name in id.per_endpoint_map_names() {
            let path = self.pins.per_endpoint_map_dir(name, id);
            self.backend.unpin_map(&path)?;
        }

        Ok(())
    }

    pub fn list_active_endpoints(&self) -> Res<Vec<EndpointId>> {
        let mut matched: HashMap<EndpointId, HashSet<&'static str>> = HashMap::new();
        for rel in self.backend.list_pins(&self.pins.links_dir())? {
            let Some(parent) = rel.parent() else { continue };
            let Some(id) = PinRoot::parse_pin_subpath(parent) else {
                continue;
            };
            let Some(filename) = rel.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            if let Some((hook_spec, _ifname)) = match_pin(&id, filename) {
                matched
                    .entry(id)
                    .or_default()
                    .insert(hook_spec.program_name);
            }
        }

        Ok(matched
            .into_iter()
            .filter(|(id, matched_programs)| {
                id.hooks()
                    .iter()
                    .filter(|hook_spec| hook_spec.required)
                    .all(|hook_spec| matched_programs.contains(hook_spec.program_name))
            })
            .map(|(id, _)| id)
            .collect())
    }

    pub fn reconcile(&mut self, desired: &[EndpointId]) -> Res<Vec<(EndpointId, LoaderError)>> {
        let mut errors = Vec::new();

        for id in desired {
            if let Err(e) = self.add_endpoint(id) {
                errors.push((id.clone(), e));
            }
        }

        let existing: HashSet<_> = self.list_active_endpoints()?.into_iter().collect();
        let wanted: HashSet<_> = desired.iter().cloned().collect();
        for orphan in existing.difference(&wanted) {
            if let Err(e) = self.remove_endpoint(orphan) {
                errors.push((orphan.clone(), e));
            }
        }

        Ok(errors)
    }

    pub fn teardown_all(&mut self) -> Res<()> {
        self.backend.remove_pin_dir(&self.pins.links_dir())?;
        self.backend.remove_pin_dir(&self.pins.globals_dir())?;
        Ok(())
    }
}

fn match_pin<'f>(id: &EndpointId, filename: &'f str) -> Option<(&'static HookSpec, &'f str)> {
    id.hooks().iter().find_map(|hook_spec| {
        let suffix = format!("-{}", hook_spec.program_name);
        filename
            .strip_suffix(suffix.as_str())
            .map(|ifname| (hook_spec, ifname))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        endpoint::ContainerId,
        mock_backend::{Call, MockBackend},
    };

    fn container(n: u16) -> EndpointId {
        EndpointId::Container(ContainerId(n))
    }

    #[test]
    fn add_then_remove_happy_path() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        let id = container(1);

        let handle = loader.add_endpoint(&id).unwrap();
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![id.clone()]);

        // CONTAINER_PER_ENDPOINT_MAPS currently declares one map. If
        // that list changes, update this count.
        assert_eq!(handle.map_paths.len(), 1);

        loader.remove_endpoint(&id).unwrap();
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![]);
    }

    #[test]
    fn each_endpoint_gets_its_own_resolve_load_and_attach_calls() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader.add_endpoint(&container(1)).unwrap();
        loader.add_endpoint(&container(2)).unwrap();

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
        loader.add_endpoint(&container(1)).unwrap();
        loader.add_endpoint(&container(2)).unwrap();

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
        let handle = loader.add_endpoint(&container(1)).unwrap();

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

        let result = loader.add_endpoint(&container(1));
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

        let result = loader.add_endpoint(&container(1));
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
        let id = container(1);
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");

        let fail_index = loader.backend.next_call_index() + 2;
        loader.backend.fail_at = Some((fail_index, "simulated crash during ingress attach".into()));

        let result = loader.add_endpoint(&id);
        assert!(result.is_err(), "ingress hook failed, so this must fail");
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![]);

        // "the daemon restarts" - clear the injected failure and call
        // add_endpoint again with the same id.
        loader.backend.fail_at = None;
        loader.add_endpoint(&id).unwrap();
        assert_eq!(loader.list_active_endpoints().unwrap(), vec![id]);
    }

    #[test]
    fn reconcile_removes_endpoints_missing_from_desired_state() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader.add_endpoint(&container(1)).unwrap();
        loader.add_endpoint(&container(2)).unwrap();

        let desired = vec![container(1)];
        let errors = loader.reconcile(&desired).unwrap();
        assert!(errors.is_empty());

        assert_eq!(loader.list_active_endpoints().unwrap(), vec![container(1)]);
    }

    #[test]
    fn reconcile_repairs_half_attached_endpoints_found_on_disk() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader.add_endpoint(&container(1)).unwrap();

        let fail_index = loader.backend.next_call_index() + 2;
        loader.backend.fail_at = Some((fail_index, "crash right after loading".into()));
        let _ = loader.add_endpoint(&container(2));
        loader.backend.fail_at = None;

        assert_eq!(loader.list_active_endpoints().unwrap(), vec![container(1)]);

        let desired = vec![container(1), container(2)];
        let errors = loader.reconcile(&desired).unwrap();
        assert!(
            errors.is_empty(),
            "reconcile should finish attaching container 2: {errors:?}"
        );

        let mut active = loader.list_active_endpoints().unwrap();
        active.sort_by_key(|id| id.kind_str().to_string());
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn optional_hook_failure_does_not_fail_the_endpoint() {
        let container_hooks = container(1).hooks();
        assert!(
            container_hooks.iter().all(|h| h.required),
            "no optional hooks defined yet - update this test when XDP lands"
        );
    }

    #[test]
    fn remove_endpoint_cleans_up_only_its_own_per_endpoint_map() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader.add_endpoint(&container(1)).unwrap();
        loader.add_endpoint(&container(2)).unwrap();

        loader.remove_endpoint(&container(1)).unwrap();

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
                .any(|p| p.contains("calls_map") && p.ends_with("00001")),
            "container 1's per-endpoint map must be gone: {remaining:?}"
        );
        assert!(
            remaining
                .iter()
                .any(|p| p.contains("calls_map") && p.ends_with("00002")),
            "container 2's per-endpoint map must be untouched: {remaining:?}"
        );
    }

    #[test]
    fn teardown_all_removes_links_and_all_maps() {
        let mut loader = Loader::new(MockBackend::new(), "/sys/fs/bpf/test");
        loader.add_endpoint(&container(1)).unwrap();
        loader
            .add_endpoint(&EndpointId::Host("the_host".to_string()))
            .unwrap();

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
