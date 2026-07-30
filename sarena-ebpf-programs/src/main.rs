#![no_std]
#![no_main]
#![allow(nonstandard_style, dead_code)]

use aya_ebpf::{
    bindings::tcx_action_base::TCX_NEXT,
    macros::{classifier, map},
    maps::Array,
    programs::TcContext,
};
#[cfg(not(test))]
use sarena_ebpf_programs::do_panic;

#[map(name = "interface_config")]
static GLOBAL: Array<u32> = Array::pinned(64, 0);

#[map(name = "calls_map")]
static PER_ENDPOINT_CALLS_MAP: Array<u32> = Array::pinned(64, 0);

#[classifier]
pub fn from_container(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn to_container(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn from_host(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn to_host(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn from_netdev(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn to_netdev(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn from_overlay(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn to_overlay(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn from_wireguard(_ctx: TcContext) -> i32 {
    TCX_NEXT
}

#[classifier]
pub fn to_wireguard(_ctx: TcContext) -> i32 {
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
