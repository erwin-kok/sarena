use aya_ebpf::{macros::classifier, programs::TcContext};

#[classifier]
pub fn dummy_test(_ctx: TcContext) -> i32 {
    0
}
