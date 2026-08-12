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
