#![allow(nonstandard_style, dead_code)]

use aya_ebpf::{macros::map, maps::ProgramArray, programs::TcContext};
use sarena_shared_test::{
    FROM_CONTAINER, FROM_HOST, FROM_NETDEV, FROM_OVERLAY, FROM_WIREGUARD, TO_CONTAINER, TO_HOST,
    TO_NETDEV, TO_OVERLAY, TO_WIREGUARD, TestStatus,
};

#[map(name = "entry_call_map")]
static entry_call_map: ProgramArray = ProgramArray::with_max_entries(10, 0);

pub fn container_receive_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, FROM_CONTAINER);
    }

    TestStatus::FrameworkError
}

pub fn container_send_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, TO_CONTAINER);
    }

    TestStatus::FrameworkError
}

pub fn host_receive_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, TO_HOST);
    }

    TestStatus::FrameworkError
}

pub fn host_send_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, FROM_HOST);
    }

    TestStatus::FrameworkError
}

pub fn netdev_receive_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, FROM_NETDEV);
    }

    TestStatus::FrameworkError
}

pub fn netdev_send_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, TO_NETDEV);
    }

    TestStatus::FrameworkError
}

pub fn overlay_receive_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, FROM_OVERLAY);
    }

    TestStatus::FrameworkError
}

pub fn overlay_send_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, TO_OVERLAY);
    }

    TestStatus::FrameworkError
}

pub fn wireguard_receive_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, FROM_WIREGUARD);
    }

    TestStatus::FrameworkError
}

pub fn wireguard_send_packet(ctx: TcContext) -> TestStatus {
    unsafe {
        entry_call_map.tail_call(&ctx, TO_WIREGUARD);
    }

    TestStatus::FrameworkError
}
