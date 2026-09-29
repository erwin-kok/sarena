use libc::IFNAMSIZ;
use sha2::{Digest, Sha256};

const HOST_INTERFACE_PREFIX: &str = "lxc";
const TEMPORARY_INTERFACE_PREFIX: &str = "tmp";

/// Returns the host interface name for the given endpoint ID.
pub fn endpoint_to_ifname(endpoint_id: &str) -> String {
    let sum = hex::encode(Sha256::digest(endpoint_id.as_bytes()));
    // The returned name must stay under `IFNAMSIZ`.
    let truncate_len = IFNAMSIZ - TEMPORARY_INTERFACE_PREFIX.len() - 1;
    format!("{HOST_INTERFACE_PREFIX}{}", truncate(&sum, truncate_len))
}

/// Returns the temporary interface name for the given endpoint ID, used
/// while setting up the real interface.
pub fn endpoint_to_temp_ifname(endpoint_id: &str) -> String {
    format!("{TEMPORARY_INTERFACE_PREFIX}{}", truncate(endpoint_id, 5))
}

/// Truncates `s` to at most `max_len` bytes, snapping down to the nearest
/// `char` boundary if `max_len` doesn't land on one.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        return s;
    }
    let mut end = max_len;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
