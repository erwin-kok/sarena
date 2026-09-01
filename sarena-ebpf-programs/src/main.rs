#![no_std]
#![no_main]
#![allow(nonstandard_style, dead_code)]

use aya_ebpf::{
    macros::{classifier, map},
    maps::Array,
    programs::TcContext,
};
use sarena_ebpf_programs::dispatch;
#[cfg(not(test))]
use sarena_ebpf_programs::do_panic;

#[map(name = "interface_config")]
static GLOBAL: Array<u32> = Array::pinned(64, 0);

#[map(name = "calls_map")]
static PER_ENDPOINT_CALLS_MAP: Array<u32> = Array::with_max_entries(64, 0);

#[classifier]
pub fn from_container(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_from_container(ctx))
}

#[classifier]
pub fn to_container(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_to_container(ctx))
}

#[classifier]
pub fn from_host(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_from_host(ctx))
}

#[classifier]
pub fn to_host(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_to_host(ctx))
}

#[classifier]
pub fn from_netdev(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_from_netdev(ctx))
}

#[classifier]
pub fn to_netdev(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_to_netdev(ctx))
}

#[classifier]
pub fn from_overlay(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_from_overlay(ctx))
}

#[classifier]
pub fn to_overlay(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_to_overlay(ctx))
}

#[classifier]
pub fn from_wireguard(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_from_wireguard(ctx))
}

#[classifier]
pub fn to_wireguard(ctx: TcContext) -> i32 {
    dispatch(sarena_ebpf_programs::try_to_wireguard(ctx))
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    do_panic(info)
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
