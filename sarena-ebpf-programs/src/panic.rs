use core::panic::PanicInfo;

pub fn do_panic(_info: &PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
