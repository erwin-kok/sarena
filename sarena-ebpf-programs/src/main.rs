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
pub fn from_container(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_from_container(ctx) {
        Ok(ret) => ret,
        Err(()) => TCX_NEXT,
    }
}

#[classifier]
pub fn to_container(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_to_container(ctx) {
        Ok(ret) => ret,
        Err(()) => TCX_NEXT,
    }
}

#[classifier]
pub fn from_host(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_from_host(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[classifier]
pub fn to_host(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_to_host(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[classifier]
pub fn from_netdev(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_from_netdev(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[classifier]
pub fn to_netdev(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_to_netdev(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[classifier]
pub fn from_overlay(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_from_overlay(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[classifier]
pub fn to_overlay(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_to_overlay(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[classifier]
pub fn from_wireguard(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_from_wireguard(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[classifier]
pub fn to_wireguard(ctx: TcContext) -> i32 {
    match sarena_ebpf_programs::try_to_wireguard(ctx) {
        Ok(ret) => ret,
        Err(_) => TCX_NEXT,
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    do_panic(info)
}

#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 13] = *b"Dual MIT/GPL\0";
