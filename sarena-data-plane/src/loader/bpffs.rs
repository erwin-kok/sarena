//! Making sure the BPF filesystem is mounted where the datapath pins its
//! maps and programs.
//!
//! Pinning only works on bpffs. Without this check a missing mount surfaces
//! much later as a confusing error: on a bare host `/sys/fs/bpf` is plain
//! sysfs and `mkdir` fails, inside a container it is overlay/tmpfs and the
//! directories are created fine but the first pin fails.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use nix::{
    mount::{MsFlags, mount},
    sys::statfs::{BPF_FS_MAGIC, statfs},
};
use thiserror::Error;
use tracing::{info, warn};

const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const BPFFS_TYPE: &str = "bpf";

#[derive(Debug, Error)]
pub enum BpffsError {
    #[error("pin root {0} must be an absolute path below a bpffs mount point")]
    InvalidPinRoot(PathBuf),

    #[error("failed to read {MOUNTINFO_PATH}: {0}")]
    MountInfo(#[source] io::Error),

    #[error("{0} exists but is not a directory")]
    NotADirectory(PathBuf),

    #[error("failed to create bpffs mount point {path}: {source}")]
    CreateMountPoint {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to mount bpffs at {path}: {source}")]
    Mount {
        path: PathBuf,
        #[source]
        source: nix::Error,
    },

    #[error("failed to statfs {path}: {source}")]
    Statfs {
        path: PathBuf,
        #[source]
        source: nix::Error,
    },

    #[error(
        "{path} is mounted with filesystem `{fstype}` instead of bpffs; mount bpffs on the \
         host (e.g. `mount -t bpf bpf {path}`) and make it visible to sarena"
    )]
    OtherFilesystem { path: PathBuf, fstype: String },

    #[error(
        "{count} bpffs mounts are stacked on {path}; each is a separate instance, so pins in the \
         lower ones are hidden. Unmount the duplicates"
    )]
    MultipleMounts { path: PathBuf, count: usize },
}

/// Checks that the bpffs mount point holding `pin_root` (its parent
/// directory, e.g. `/sys/fs/bpf` for `/sys/fs/bpf/sarena`) has exactly one
/// bpffs mounted on it, mounting one when nothing is mounted there.
///
/// A mount point that has some other filesystem is an error rather than
/// something to fall back from: a bpffs mounted elsewhere from inside the
/// daemon's pod would be gone after a restart, taking all pins with it.
pub(crate) fn ensure_bpffs(pin_root: &Path) -> Result<(), BpffsError> {
    let mount_point = mount_point_for(pin_root)?;

    let mounts = read_mounts()?;
    match top_mount_at(&mounts, &mount_point) {
        None => mount_bpffs(&mount_point)?,
        Some(m) if m.fstype != BPFFS_TYPE => {
            return Err(BpffsError::OtherFilesystem {
                path: mount_point,
                fstype: m.fstype.clone(),
            });
        }
        Some(_) => info!(
            "detected mounted BPF filesystem at {}",
            mount_point.display()
        ),
    }

    // Authoritative check of what actually sits at the mount point now.
    let stat = statfs(&mount_point).map_err(|source| BpffsError::Statfs {
        path: mount_point.clone(),
        source,
    })?;
    if stat.filesystem_type() != BPF_FS_MAGIC {
        return Err(BpffsError::OtherFilesystem {
            path: mount_point,
            fstype: format!("{:#x}", stat.filesystem_type().0),
        });
    }

    let count = count_bpffs_mounts_at(&read_mounts()?, &mount_point);
    if count > 1 {
        return Err(BpffsError::MultipleMounts {
            path: mount_point,
            count,
        });
    }

    Ok(())
}

fn mount_point_for(pin_root: &Path) -> Result<PathBuf, BpffsError> {
    let invalid = || BpffsError::InvalidPinRoot(pin_root.to_path_buf());
    if !pin_root.is_absolute() {
        return Err(invalid());
    }
    let parent = pin_root
        .parent()
        .filter(|p| *p != Path::new("/"))
        .ok_or_else(invalid)?;
    // mountinfo lists resolved paths; follow symlinks when the directory
    // exists so e.g. a symlinked `/sys/fs/bpf` still matches.
    Ok(fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf()))
}

fn mount_bpffs(mount_point: &Path) -> Result<(), BpffsError> {
    warn!(
        "BPF filesystem is not mounted at {path}, mounting it. Pinned maps will not survive a \
         restart of the daemon's pod unless bpffs is mounted on the host as well",
        path = mount_point.display()
    );

    match fs::metadata(mount_point) {
        Ok(meta) if !meta.is_dir() => {
            return Err(BpffsError::NotADirectory(mount_point.to_path_buf()));
        }
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(mount_point).map_err(|source| BpffsError::CreateMountPoint {
                path: mount_point.to_path_buf(),
                source,
            })?;
        }
        Err(source) => {
            return Err(BpffsError::CreateMountPoint {
                path: mount_point.to_path_buf(),
                source,
            });
        }
    }

    mount(
        Some(BPFFS_TYPE),
        mount_point,
        Some(BPFFS_TYPE),
        MsFlags::empty(),
        None::<&str>,
    )
    .map_err(|source| BpffsError::Mount {
        path: mount_point.to_path_buf(),
        source,
    })?;

    info!("mounted BPF filesystem at {}", mount_point.display());
    Ok(())
}

/// The fields of a `/proc/self/mountinfo` line this module needs.
#[derive(Debug, PartialEq, Eq)]
struct MountInfo {
    /// Root of the mount within its filesystem (`/` unless it's a bind
    /// mount of a subdirectory).
    root: PathBuf,
    mount_point: PathBuf,
    fstype: String,
}

fn read_mounts() -> Result<Vec<MountInfo>, BpffsError> {
    let content = fs::read_to_string(MOUNTINFO_PATH).map_err(BpffsError::MountInfo)?;
    Ok(parse_mountinfo(&content))
}

/// Parses `/proc/<pid>/mountinfo`, see proc(5):
/// `36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw`.
/// Malformed lines are skipped.
fn parse_mountinfo(content: &str) -> Vec<MountInfo> {
    content
        .lines()
        .filter_map(|line| {
            let (pre, post) = line.split_once(" - ")?;
            let mut pre = pre.split(' ');
            let root = pre.nth(3)?;
            let mount_point = pre.next()?;
            let fstype = post.split(' ').next()?;
            Some(MountInfo {
                root: PathBuf::from(unescape(root)),
                mount_point: PathBuf::from(unescape(mount_point)),
                fstype: unescape(fstype),
            })
        })
        .collect()
}

/// Undoes the kernel's octal escaping of space, tab, newline and backslash
/// (`\040`, `\011`, `\012`, `\134`) in mountinfo fields.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && let Some(code) = bytes
                .get(i + 1..i + 4)
                .and_then(|oct| std::str::from_utf8(oct).ok())
                .and_then(|oct| u8::from_str_radix(oct, 8).ok())
        {
            out.push(code);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The mount visible at `mount_point`: mountinfo lists mounts in mount
/// order, so the last match is the one on top.
fn top_mount_at<'a>(mounts: &'a [MountInfo], mount_point: &Path) -> Option<&'a MountInfo> {
    mounts.iter().rev().find(|m| m.mount_point == mount_point)
}

fn count_bpffs_mounts_at(mounts: &[MountInfo], mount_point: &Path) -> usize {
    mounts
        .iter()
        .filter(|m| {
            m.mount_point == mount_point && m.fstype == BPFFS_TYPE && m.root == Path::new("/")
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTINFO: &str = "\
22 1 0:21 / /sys rw,nosuid,nodev,noexec,relatime shared:2 - sysfs sysfs rw
30 22 0:26 / /sys/fs/bpf rw,nosuid,nodev,noexec,relatime shared:9 - bpf bpf rw,mode=700
31 22 0:27 / /sys/fs/bpf rw,relatime - bpf bpf rw
32 1 0:28 /data /mnt/with\\040space rw - ext4 /dev/sda1 rw
garbage line without separator
";

    #[test]
    fn parses_mountinfo_fields() {
        let mounts = parse_mountinfo(MOUNTINFO);
        assert_eq!(mounts.len(), 4, "the malformed line must be skipped");
        assert_eq!(
            mounts[1],
            MountInfo {
                root: PathBuf::from("/"),
                mount_point: PathBuf::from("/sys/fs/bpf"),
                fstype: "bpf".to_string(),
            }
        );
        assert_eq!(mounts[3].root, PathBuf::from("/data"));
        assert_eq!(mounts[3].mount_point, PathBuf::from("/mnt/with space"));
    }

    #[test]
    fn top_mount_is_the_last_one_listed() {
        let mounts = parse_mountinfo(MOUNTINFO);
        let top = top_mount_at(&mounts, Path::new("/sys/fs/bpf")).expect("mounted");
        assert_eq!(top.fstype, "bpf");
        assert!(top_mount_at(&mounts, Path::new("/sys/fs")).is_none());
    }

    #[test]
    fn counts_stacked_bpffs_mounts() {
        let mounts = parse_mountinfo(MOUNTINFO);
        assert_eq!(count_bpffs_mounts_at(&mounts, Path::new("/sys/fs/bpf")), 2);
        assert_eq!(count_bpffs_mounts_at(&mounts, Path::new("/sys")), 0);
    }

    #[test]
    fn unescape_leaves_plain_and_malformed_escapes_alone() {
        assert_eq!(unescape("/plain/path"), "/plain/path");
        assert_eq!(unescape("a\\134b"), "a\\b");
        assert_eq!(unescape("trailing\\04"), "trailing\\04");
    }

    #[test]
    fn mount_point_is_the_parent_of_the_pin_root() {
        let mp = mount_point_for(Path::new("/nonexistent-sarena-test/bpf/sarena")).unwrap();
        assert_eq!(mp, PathBuf::from("/nonexistent-sarena-test/bpf"));
    }

    #[test]
    fn rejects_pin_roots_without_a_usable_mount_point() {
        assert!(mount_point_for(Path::new("relative/sarena")).is_err());
        assert!(mount_point_for(Path::new("/sarena")).is_err());
        assert!(mount_point_for(Path::new("/")).is_err());
    }
}
