use nix::unistd::getuid;

pub fn require_root_privilege(cmd: &str) {
    if !getuid().is_root() {
        panic!("Please run {:?} command with root privileges.", cmd);
    }
}
