use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    DataPlaneError, EndpointId, EndpointPersist, Res,
    endpoint::id_allocator::{MAX_ID, MIN_ID},
};

pub(crate) const NEXT_DIRECTORY_SUFFIX: &str = "_next";
pub(crate) const NEXT_FAILED_DIRECTORY_SUFFIX: &str = "_next_fail";
pub(crate) const BACKUP_DIRECTORY_SUFFIX: &str = "_stale";

pub(crate) fn state_directory_path(state_dir: &Path, id: EndpointId) -> PathBuf {
    state_dir.join(id.to_string())
}

pub(crate) fn next_directory_path(state_dir: &Path, id: EndpointId) -> PathBuf {
    state_dir.join(format!("{id}{NEXT_DIRECTORY_SUFFIX}"))
}

pub(crate) fn failed_directory_path(state_dir: &Path, id: EndpointId) -> PathBuf {
    state_dir.join(format!("{id}{NEXT_FAILED_DIRECTORY_SUFFIX}"))
}

pub(crate) fn backup_directory_path(state_dir: &Path, id: EndpointId) -> PathBuf {
    state_dir.join(format!("{id}{BACKUP_DIRECTORY_SUFFIX}"))
}

pub(crate) fn remove_dir_if_exists(dir: &PathBuf) -> std::io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[allow(dead_code)]
pub(crate) fn remove_directories(state_dir: &Path, id: EndpointId) {
    let _ = remove_dir_if_exists(&state_directory_path(state_dir, id));
    let _ = remove_dir_if_exists(&next_directory_path(state_dir, id));
    let _ = remove_dir_if_exists(&failed_directory_path(state_dir, id));
    let _ = remove_dir_if_exists(&backup_directory_path(state_dir, id));
}

pub(crate) fn synchronize_directories(state_dir: &Path, id: EndpointId) -> Res<()> {
    let current_dir = state_directory_path(state_dir, id);
    let next_dir = next_directory_path(state_dir, id);
    let backup_dir = backup_directory_path(state_dir, id);

    remove_dir_if_exists(&backup_dir)?;
    let had_current = match fs::rename(&current_dir, &backup_dir) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };

    if let Err(e) = fs::rename(&next_dir, &current_dir) {
        if had_current && let Err(rollback_err) = fs::rename(&backup_dir, &current_dir) {
            // The previous state now only lives in the backup directory;
            // read_endpoints() falls back to it on restart.
            tracing::error!(
                endpoint_id = %id,
                error = %rollback_err,
                "Failed to restore {} from {}",
                current_dir.display(),
                backup_dir.display()
            );
        }
        return Err(e.into());
    }

    // The new state is committed at this point, so a failed cleanup must not fail the
    // regeneration. A leftover backup is removed by the next synchronization or restore.
    if let Err(err) = remove_dir_if_exists(&backup_dir) {
        tracing::warn!(
            endpoint_id = %id,
            error = %err,
            "Failed to remove backup directory {}",
            backup_dir.display()
        );
    }
    Ok(())
}

/// The kinds of state directory an endpoint can have. The derived order is the restore
/// preference: the first one that can be read wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DirKind {
    /// `<id>`: the committed state.
    Current,
    /// `<id>_next`: only left behind (without `<id>`) by a crash during regeneration. Note
    /// that a readable `_next` does not mean the regeneration completed: the state file is
    /// written before the BPF programs are regenerated, so it may hold state that was never
    /// applied.
    Next,
    /// `<id>_stale`: the previously committed state, left behind when
    /// `synchronize_directories()` was interrupted or failed to roll back.
    Backup,
    /// `<id>_next_fail`: the state of a failed regeneration.
    NextFailed,
}

pub(crate) fn read_endpoints(state_dir: &Path) -> Res<HashMap<EndpointId, EndpointPersist>> {
    let mut endpoints = HashMap::new();

    for (id, mut candidates) in collect_endpoint_dirs(state_dir)? {
        candidates.sort_by_key(|(kind, _)| *kind);

        let mut chosen = None;
        for (index, (_, path)) in candidates.iter().enumerate() {
            match read_endpoint_dir(id, path) {
                Ok(endpoint) => {
                    endpoints.insert(id, endpoint);
                    chosen = Some(index);
                    break;
                }
                Err(err) => {
                    tracing::warn!(
                        endpoint_id = %id,
                        error = %err,
                        "Failed to read endpoint state from {}. Ignoring it.",
                        path.display()
                    );
                }
            }
        }

        // Without a usable directory, leave everything on disk for inspection.
        let Some(chosen) = chosen else {
            continue;
        };

        // Remove the other directories of this endpoint.
        for (index, (_, path)) in candidates.iter().enumerate() {
            if index == chosen {
                continue;
            }
            tracing::warn!(
                endpoint_id = %id,
                "Removing superseded state directory {}",
                path.display()
            );
            if let Err(err) = remove_dir_if_exists(path) {
                tracing::warn!(
                    endpoint_id = %id,
                    error = %err,
                    "Error while removing directory {}. Ignoring it...",
                    path.display()
                );
            }
        }
    }

    Ok(endpoints)
}

fn read_endpoint_dir(id: EndpointId, path: &Path) -> Res<EndpointPersist> {
    let endpoint = EndpointPersist::read_state(path)?;
    if endpoint.id != id {
        return Err(DataPlaneError::GeneralError(format!(
            "state file has endpoint id {}, expected {id}",
            endpoint.id
        )));
    }
    Ok(endpoint)
}

/// Groups the state directories in `state_dir` by endpoint id. Entries whose name is not a
/// state directory name are ignored.
fn collect_endpoint_dirs(state_dir: &Path) -> Res<HashMap<EndpointId, Vec<(DirKind, PathBuf)>>> {
    let entries = match fs::read_dir(state_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e.into()),
    };

    let mut dirs: HashMap<EndpointId, Vec<(DirKind, PathBuf)>> = HashMap::new();
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Some((id, kind)) = entry.file_name().to_str().and_then(parse_dir_name) else {
            continue;
        };
        dirs.entry(id).or_default().push((kind, entry.path()));
    }
    Ok(dirs)
}

/// Parses `<id>[suffix]`. The id must be in canonical form (e.g. not `+12` or `012`) and
/// within the range of allocatable endpoint ids.
fn parse_dir_name(name: &str) -> Option<(EndpointId, DirKind)> {
    let (base, kind) = if let Some(base) = name.strip_suffix(NEXT_FAILED_DIRECTORY_SUFFIX) {
        (base, DirKind::NextFailed)
    } else if let Some(base) = name.strip_suffix(NEXT_DIRECTORY_SUFFIX) {
        (base, DirKind::Next)
    } else if let Some(base) = name.strip_suffix(BACKUP_DIRECTORY_SUFFIX) {
        (base, DirKind::Backup)
    } else {
        (name, DirKind::Current)
    };

    let id: EndpointId = base.parse().ok()?;
    (id.to_string() == base && (MIN_ID..=MAX_ID).contains(&id)).then_some((id, kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EndpointSpec;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sarena-dir-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes a state directory `name` holding endpoint `id`, tagged with `marker` so the
    /// test can tell which directory was restored.
    fn write_dir(state_dir: &Path, name: &str, id: u16, marker: &str) {
        let dir = state_dir.join(name);
        fs::create_dir_all(&dir).unwrap();
        let mut spec = EndpointSpec::default();
        spec.container_id = Some(marker.to_string());
        EndpointPersist {
            id: EndpointId::new(id),
            spec,
        }
        .write_state(&dir)
        .unwrap();
    }

    fn marker(endpoints: &HashMap<EndpointId, EndpointPersist>, id: u16) -> &str {
        endpoints[&EndpointId::new(id)]
            .container_id
            .as_deref()
            .unwrap()
    }

    #[test]
    fn missing_state_dir_is_empty() {
        let dir = std::env::temp_dir().join("sarena-dir-does-not-exist");
        assert!(read_endpoints(&dir).unwrap().is_empty());
    }

    #[test]
    fn prefers_current_and_removes_others() {
        let dir = temp_dir("current");
        write_dir(&dir, "7", 7, "current");
        write_dir(&dir, "7_next", 7, "next");
        write_dir(&dir, "7_next_fail", 7, "fail");
        write_dir(&dir, "7_stale", 7, "stale");

        let endpoints = read_endpoints(&dir).unwrap();
        assert_eq!(marker(&endpoints, 7), "current");
        assert!(dir.join("7").exists());
        for name in ["7_next", "7_next_fail", "7_stale"] {
            assert!(!dir.join(name).exists(), "{name} should be removed");
        }
    }

    #[test]
    fn prefers_next_over_next_fail() {
        let dir = temp_dir("next");
        write_dir(&dir, "7_next_fail", 7, "fail");
        write_dir(&dir, "7_next", 7, "next");

        assert_eq!(marker(&read_endpoints(&dir).unwrap(), 7), "next");
        assert!(!dir.join("7_next_fail").exists());
    }

    #[test]
    fn falls_back_to_backup() {
        let dir = temp_dir("backup");
        write_dir(&dir, "7_stale", 7, "stale");
        write_dir(&dir, "7_next_fail", 7, "fail");

        assert_eq!(marker(&read_endpoints(&dir).unwrap(), 7), "stale");
    }

    #[test]
    fn skips_unreadable_and_mismatched_dirs() {
        let dir = temp_dir("unreadable");
        fs::create_dir_all(dir.join("7")).unwrap(); // no state file
        write_dir(&dir, "7_next", 8, "wrong-id");
        write_dir(&dir, "7_stale", 7, "stale");
        // Nothing usable for endpoint 9: keep it on disk.
        fs::create_dir_all(dir.join("9")).unwrap();
        fs::create_dir_all(dir.join("9_next")).unwrap();

        let endpoints = read_endpoints(&dir).unwrap();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(marker(&endpoints, 7), "stale");
        assert!(dir.join("9").exists() && dir.join("9_next").exists());
    }

    #[test]
    fn ignores_invalid_names() {
        let dir = temp_dir("names");
        for name in ["foo_next", "+7", "07", "0", "4096", "7_other"] {
            write_dir(&dir, name, 7, name);
        }

        assert!(read_endpoints(&dir).unwrap().is_empty());
        assert!(dir.join("foo_next").exists());
    }
}
