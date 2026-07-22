#![no_std]
#![no_main]
#![allow(nonstandard_style, dead_code)]

#[cfg(not(test))]
use sarena_ebpf_test_programs::do_panic;

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    do_panic(info)
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
