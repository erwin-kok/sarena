/// Global TCP conntrack buffer (`LruHashMap<ConnTrackKey, ConnTrackEntry>`).
pub const CONNTRACK_TCP_MAP: &str = "conntrack_tcp_buffer";

/// Global non-TCP conntrack buffer (`LruHashMap<ConnTrackKey, ConnTrackEntry>`).
pub const CONNTRACK_ANY_MAP: &str = "conntrack_any_buffer";
