use network_types::ip::IpProto;

pub const TIMEOUT_TCP_NEW: u64 = 30 * 1_000_000_000;
pub const TIMEOUT_TCP_ESTABLISHED: u64 = 6 * 3600 * 1_000_000_000;
pub const TIMEOUT_TCP_CLOSING: u64 = 10 * 1_000_000_000;
pub const TIMEOUT_DEFAULT: u64 = 60 * 1_000_000_000;

/// Timeout for a brand-new entry, before ct_update() has seen it settle.
pub fn initial_timeout(proto: u8) -> u64 {
    if proto == IpProto::Tcp as u8 {
        TIMEOUT_TCP_NEW
    } else {
        TIMEOUT_DEFAULT
    }
}
