use aya_ebpf::programs::TcContext;

use crate::error::{CommonError::PacketSizeError, Res};

#[inline(always)]
pub unsafe fn ptr_at<T>(ctx: &TcContext, offset: usize) -> Res<*const T> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len: usize = core::mem::size_of::<T>();

    if start + offset + len > end {
        return Err(PacketSizeError(core::any::type_name::<T>()));
    }

    Ok((start + offset) as *const T)
}

#[inline(always)]
pub unsafe fn mut_ptr_at<T>(ctx: &TcContext, offset: usize) -> Res<*mut T> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len: usize = core::mem::size_of::<T>();

    if start + offset + len > end {
        return Err(PacketSizeError(core::any::type_name::<T>()));
    }

    Ok((start + offset) as *mut T)
}

/// Bounds-checked shared reference to a `T` at `offset` bytes into the packet.
///
/// # Safety
/// The reference borrows packet memory, so:
/// * it must not be held across any helper that can reallocate the skb (`bpf_skb_store_bytes`,
///   `l3_csum_replace`, `l4_csum_replace`, `bpf_skb_adjust_room`, `bpf_skb_pull_data`, ...):
///   re-derive afterwards;
/// * `T` must have alignment 1 (network-types headers do -- every field is a byte or byte array).
///
/// The returned lifetime is caller-chosen; keep it to a tight local scope.
#[inline(always)]
pub unsafe fn at<'a, T>(ctx: &TcContext, offset: usize) -> Res<&'a T> {
    Ok(unsafe { &*ptr_at::<T>(ctx, offset)? })
}

/// Bounds-checked mutable reference to a `T` at `offset` bytes into the packet.
///
/// # Safety
/// Same as [`at`], plus: only **one** `&mut` into the packet may be live at
/// a time (two overlapping `&mut` into one allocation is UB even if the
/// fields don't overlap). Use disjoint scopes for `eth` vs `ip4`.
#[inline(always)]
pub unsafe fn at_mut<'a, T>(ctx: &TcContext, offset: usize) -> Res<&'a mut T> {
    Ok(unsafe { &mut *mut_ptr_at::<T>(ctx, offset)? })
}
