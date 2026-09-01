use network_types::ip::Ipv4Hdr;

// If MF is set, or the frag_offset is nonzero, we have an IP fragment.
pub fn is_fragmented(ipv4: &Ipv4Hdr) -> bool {
    (ipv4.frag_flags() & 0x1) != 0 || (ipv4.frag_offset() != 0)
}
