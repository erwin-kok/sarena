/// A compiled eBPF ELF object embedded in the binary.
#[derive(Debug, Clone, Copy)]
pub struct EbpfObject {
    /// Name of the eBPF package the object was built from.
    pub name: &'static str,
    /// The ELF bytes, aligned (via `aya::include_bytes_aligned!`) so they can
    /// be parsed in place.
    pub bytes: &'static [u8],
    /// Hex-encoded SHA-256 of `bytes`, for identifying the embedded build.
    pub sha256: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/objects.rs"));

#[cfg(test)]
mod tests {
    use super::{EbpfObject, PROGRAMS, TEST_PROGRAMS};

    #[test]
    fn embedded_objects_are_aligned_elf() {
        for object in [PROGRAMS, TEST_PROGRAMS] {
            let EbpfObject {
                name,
                bytes,
                sha256,
            } = object;
            assert_eq!(bytes.as_ptr().align_offset(8), 0, "{name} is not aligned");
            assert_eq!(&bytes[..4], b"\x7fELF", "{name} is not an ELF object");
            assert_eq!(sha256.len(), 64, "{name} has a malformed digest");
        }
    }
}
