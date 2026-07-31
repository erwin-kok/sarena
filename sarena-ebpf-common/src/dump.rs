use aya_ebpf::{bpf_printk, programs::TcContext};

const HD_MAX_BYTES: usize = 128;

#[inline(always)]
fn nibble_to_char(nib: u8) -> u8 {
    if nib < 10 {
        b'0' + nib
    } else {
        b'a' + nib - 10
    }
}

#[inline]
pub fn dump_hex(ctx: &TcContext, msg: &str, len: usize, off: usize) {
    let pkt_len = ctx.len() as usize;

    if off >= pkt_len {
        return;
    }

    let dump_len = core::cmp::min(HD_MAX_BYTES, core::cmp::min(len, pkt_len - off));

    let mut buf = [0u8; HD_MAX_BYTES * 2 + 1];

    for i in 0..dump_len {
        let v: u8 = match ctx.load(off + i) {
            Ok(v) => v,
            Err(_) => break,
        };

        buf[i * 2] = nibble_to_char(v >> 4);
        buf[i * 2 + 1] = nibble_to_char(v & 0xf);
    }
    unsafe {
        bpf_printk!(c"%s: pkt_hex: %s", msg.as_ptr(), buf.as_ptr());
    }
}
