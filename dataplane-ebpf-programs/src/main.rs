#![no_std]
#![no_main]
#![allow(nonstandard_style, dead_code)]

use aya_ebpf::{bindings::tcx_action_base::TCX_NEXT, macros::classifier, programs::TcContext};
#[cfg(not(test))]
use dataplane_ebpf_programs::do_panic;

#[classifier]
pub fn from_container(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    do_panic(info)
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
