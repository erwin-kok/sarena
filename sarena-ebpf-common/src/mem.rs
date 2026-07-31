#[inline(always)]
pub fn bpf_memcpy(dst: *mut u8, src: *const u8, len: usize) {
    unsafe {
        for i in 0..len {
            let byte = core::ptr::read(src.wrapping_add(i));
            core::ptr::write_volatile(dst.wrapping_add(i), byte);
        }
    }
}

/// Same signature/semantics as libc's `memcmp`: compares `n` bytes at `s1`
/// and `s2`, returning 0 if equal, or the difference (as `unsigned char`,
/// i.e. `u8`) between the first mismatching byte pair otherwise.
#[inline(always)]
pub fn bpf_memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    unsafe {
        for i in 0..n {
            let a = core::ptr::read(s1.wrapping_add(i));
            let b = core::ptr::read(s2.wrapping_add(i));
            if a != b {
                return a as i32 - b as i32;
            }
        }
    }
    0
}
