pub fn require_root_privilege() {
    let euid = unsafe { libc::geteuid() };
    assert!(euid == 0, "this command requires root privilege")
}
