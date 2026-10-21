use std::path::PathBuf;

use crate::EndpointId;

pub(crate) const NEXT_DIRECTORY_SUFFIX: &str = "_next";
pub(crate) const NEXT_FAILED_DIRECTORY_SUFFIX: &str = "_next_fail";
pub(crate) const BACKUP_DIRECTORY_SUFFIX: &str = "_stale";

pub(crate) fn state_directory_path(state_dir: &PathBuf, ep: EndpointId) -> PathBuf {
    state_dir.join(ep.to_string())
}

pub(crate) fn next_directory_path(state_dir: &PathBuf, ep: EndpointId) -> PathBuf {
    state_dir.join(format!("{}{}", ep.to_string(), NEXT_DIRECTORY_SUFFIX))
}

pub(crate) fn failed_directory_path(state_dir: &PathBuf, ep: EndpointId) -> PathBuf {
    state_dir.join(format!(
        "{}{}",
        ep.to_string(),
        NEXT_FAILED_DIRECTORY_SUFFIX
    ))
}

pub(crate) fn backup_directory_path(state_dir: &PathBuf, ep: EndpointId) -> PathBuf {
    state_dir.join(format!("{}{}", ep.to_string(), BACKUP_DIRECTORY_SUFFIX))
}

pub(crate) fn remove_dir_if_exists(dir: &PathBuf) -> std::io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub(crate) fn remove_driectories(state_dir: &PathBuf, ep: EndpointId) {
    let _ = remove_dir_if_exists(&state_directory_path(state_dir, ep));
    let _ = remove_dir_if_exists(&next_directory_path(state_dir, ep));
    let _ = remove_dir_if_exists(&failed_directory_path(state_dir, ep));
    let _ = remove_dir_if_exists(&backup_directory_path(state_dir, ep));
}
