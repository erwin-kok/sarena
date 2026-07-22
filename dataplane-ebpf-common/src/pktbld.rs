use aya_ebpf::{cty::c_long, programs::TcContext};

use crate::bpf_memcpy;

// ─── constants ───────────────────────────────────────────────────────────────

pub const PKT_BUILDER_LAYERS: usize = 7;
pub const MAX_PACKET_OFF: u64 = 0xffff;

// ─── layer enum ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PktLayer {
    None,

    // Raw payload
    Data,
}

pub struct PacketBuilder<'a> {
    pub ctx: &'a TcContext,
    pub cur_off: u64,
    pub layer_offsets: [u64; PKT_BUILDER_LAYERS],
    pub layers: [PktLayer; PKT_BUILDER_LAYERS],
}

// ─── core builder ────────────────────────────────────────────────────────────

impl<'a> PacketBuilder<'a> {
    /// Create a new packet builder attached to `ctx`.
    #[inline]
    #[must_use]
    pub fn new(ctx: &'a TcContext) -> Self {
        Self {
            ctx,
            cur_off: 0,
            layer_offsets: [0u64; PKT_BUILDER_LAYERS],
            layers: [PktLayer::None; PKT_BUILDER_LAYERS],
        }
    }

    /// Return the index of the first free (None) layer slot, or -1 if full.    
    #[inline]
    #[must_use]
    pub fn free_layer(&self) -> i32 {
        let mut i = 0usize;
        while i < PKT_BUILDER_LAYERS {
            if self.layers[i] == PktLayer::None {
                return i as i32;
            }
            i += 1;
        }
        -1
    }

    /// Reserve `len` bytes of uninitialised payload.
    #[inline]
    pub fn push_data_room(&mut self, len: i32) -> u64 {
        let needed = (self.cur_off + len as u64 - self.ctx.len() as u64) as i32;
        if ctx_adjust_room(self.ctx, needed).is_err() {
            return 0;
        }

        let data = self.ctx.data();
        let data_end = self.ctx.data_end();
        if data > data_end {
            return 0;
        }

        if self.cur_off as i64 >= MAX_PACKET_OFF as i64 - len as i64 {
            return 0;
        }

        let layer = data + self.cur_off as usize;
        let layer_idx = self.free_layer();
        if layer_idx < 0 {
            return 0;
        }

        self.layers[layer_idx as usize] = PktLayer::Data;
        self.layer_offsets[layer_idx as usize] = self.cur_off;
        self.cur_off += len as u64;
        layer as u64
    }

    /// Copy `data` into the packet as a payload.
    #[inline]
    pub fn push_data(&mut self, data: &[u8]) -> u64 {
        let pkt_data = self.push_data_room(data.len() as i32);
        if pkt_data == 0 {
            return 0;
        }

        let end = pkt_data + data.len() as u64;
        if end > self.ctx.data_end() as u64 {
            return 0;
        }

        bpf_memcpy(pkt_data as *mut u8, data.as_ptr(), data.len());

        pkt_data
    }

    #[inline]
    pub fn build(&self) {
        let mut i = 0usize;
        while i < PKT_BUILDER_LAYERS {
            match self.layers[i] {
                PktLayer::None => return, // end of stack
                _ => {}
            }
            i += 1;
        }
    }
}

pub fn ctx_adjust_room(ctx: &TcContext, len_diff: i32) -> Result<(), c_long> {
    ctx.change_tail((ctx.len() as i32 + len_diff) as u32, 0)
}
