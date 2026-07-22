#[inline(always)]
pub fn bpf_memcpy(dst: *mut u8, src: *const u8, len: usize) {
    unsafe {
        for i in 0..len {
            let byte = core::ptr::read(src.wrapping_add(i));
            core::ptr::write_volatile(dst.wrapping_add(i), byte);
        }
    }
}
