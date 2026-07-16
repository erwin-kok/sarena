use crate::{TEST_RESULT_MAP_SIZE, Tag, WIRE_VERSION};

pub struct TlvWriter<'a> {
    buf: &'a mut [u8; TEST_RESULT_MAP_SIZE],
    pos: usize,
}

impl<'a> TlvWriter<'a> {
    pub fn new(buf: &'a mut [u8; TEST_RESULT_MAP_SIZE]) -> Option<Self> {
        let mut w = Self { buf, pos: 0 };
        w.write_u8(Tag::Version, WIRE_VERSION)?;
        Some(w)
    }

    #[must_use]
    pub const fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    pub fn write_u8(&mut self, tag: Tag, val: u8) -> Option<()> {
        self.need(3)?;
        self.buf[self.pos] = tag as u8;
        self.buf[self.pos + 1] = 1;
        self.buf[self.pos + 2] = val;
        self.pos += 3;
        Some(())
    }

    #[inline]
    pub fn write_u16(&mut self, tag: Tag, val: u16) -> Option<()> {
        self.need(4)?;
        self.buf[self.pos] = tag as u8;
        self.buf[self.pos + 1] = 2;
        self.buf[self.pos + 2..self.pos + 4].copy_from_slice(&val.to_le_bytes());
        self.pos += 4;
        Some(())
    }

    #[inline]
    pub fn write_u32(&mut self, tag: Tag, val: u32) -> Option<()> {
        self.need(6)?;
        self.buf[self.pos] = tag as u8;
        self.buf[self.pos + 1] = 4;
        self.buf[self.pos + 2..self.pos + 6].copy_from_slice(&val.to_le_bytes());
        self.pos += 6;
        Some(())
    }

    #[inline]
    pub fn write_u64(&mut self, tag: Tag, val: u64) -> Option<()> {
        self.need(10)?;
        self.buf[self.pos] = tag as u8;
        self.buf[self.pos + 1] = 8;
        self.buf[self.pos + 2..self.pos + 10].copy_from_slice(&val.to_le_bytes());
        self.pos += 10;
        Some(())
    }

    #[inline]
    pub fn write_bytes(&mut self, tag: Tag, data: &[u8]) -> Option<()> {
        let len = data.len();
        if len > 255 {
            return None;
        }
        self.need(2 + len)?;
        self.buf[self.pos] = tag as u8;
        self.buf[self.pos + 1] = len as u8;
        self.buf[self.pos + 2..self.pos + 2 + len].copy_from_slice(data);
        self.pos += 2 + len;
        Some(())
    }

    #[inline]
    #[must_use]
    pub const fn finish(self) -> usize {
        self.pos
    }

    #[inline]
    const fn need(&self, n: usize) -> Option<()> {
        if self.pos + n <= TEST_RESULT_MAP_SIZE {
            Some(())
        } else {
            None
        }
    }
}
