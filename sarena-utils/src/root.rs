use nix::unistd::getuid;

pub fn require_root_privilege(cmd: &str) {
    assert!(
        getuid().is_root(),
        "Please run {cmd:?} command with root privileges.",
    );
}
