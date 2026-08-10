use aya_ebpf::{
    EbpfContext as _, bindings::__sk_buff, helpers::generated::bpf_redirect_peer,
    programs::TcContext,
};

pub fn ctx_redirect_peer(ifindex: u32, flags: u64) -> i64 {
    unsafe { bpf_redirect_peer(ifindex, flags) }
}

pub fn ctx_get_ifindex(ctx: &TcContext) -> u32 {
    let skb = ctx.as_ptr() as *const __sk_buff;
    let ifindex = unsafe { (*skb).ifindex };
    ifindex
}
