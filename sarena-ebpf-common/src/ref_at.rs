use aya_ebpf::programs::TcContext;

use crate::error::{CommonError::PacketSizeError, Res};

#[inline(always)]
pub unsafe fn ref_at<'a, T>(ctx: &TcContext, offset: usize) -> Res<&'a T> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len: usize = core::mem::size_of::<T>();

    if start + offset + len > end {
        return Err(PacketSizeError);
    }

    Ok(unsafe { &*((start + offset) as *const T) })
}

#[inline(always)]
pub unsafe fn ref_at_mut<'a, T>(ctx: &TcContext, offset: usize) -> Res<&'a mut T> {
    let start = ctx.data();
    let end = ctx.data_end();
    let len: usize = core::mem::size_of::<T>();

    if start + offset + len > end {
        return Err(PacketSizeError);
    }

    Ok(unsafe { &mut *((start + offset) as *mut T) })
}
