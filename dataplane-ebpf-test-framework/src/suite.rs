use aya_ebpf::{
    macros::map,
    maps::{Array, PerCpuArray},
    programs::TcContext,
};
use dataplane_common_test::{
    SCAPY_ASSERT_NULL, SCAPY_MAX_ASSERTS, SCAPY_MAX_STR_LEN, ScapyAssert, TEST_RESULT_MAP_SIZE,
    Tag, TestStatus, tlv_writer::TlvWriter,
};

use crate::test_log;

#[map(name = "test_suite_result")]
pub static TEST_SUITE_RESULT: Array<[u8; TEST_RESULT_MAP_SIZE]> = Array::pinned(1, 0);

#[map(name = "test_suite_status_code")]
pub static TEST_SUITE_STATUS_CODE: Array<u32> = Array::with_max_entries(1, 0);

#[map(name = "scapy_assert_map")]
pub static SCAPY_ASSERT_MAP: Array<ScapyAssert> =
    Array::with_max_entries(SCAPY_MAX_ASSERTS as u32, 0);

#[map(name = "scapy_assert_map_count")]
pub static SCAPY_ASSERT_MAP_COUNT: Array<u32> = Array::with_max_entries(1, 0);

#[map(name = "__scapy_assert_map_scratch")]
static SCAPY_ASSERT_MAP_SCRATCH: PerCpuArray<ScapyAssert> = PerCpuArray::with_max_entries(1, 0);

pub struct TestSuite<'a> {
    pub writer: TlvWriter<'a>,
    status: TestStatus,
}

impl<'a> TestSuite<'a> {
    pub fn new(name: &str, file: &str) -> Option<Self> {
        let ptr = TEST_SUITE_RESULT.get_ptr_mut(0)?;
        let buf = unsafe { &mut *ptr };
        let mut writer = TlvWriter::new(buf)?;
        writer.write_bytes(Tag::TestName, name.as_bytes())?;
        writer.write_bytes(Tag::TestFile, file.as_bytes())?;
        Some(Self {
            writer,
            status: TestStatus::Pass,
        })
    }

    #[inline]
    pub fn status(&self) -> TestStatus {
        self.status
    }

    #[inline]
    pub fn set_status(&mut self, status: TestStatus) {
        self.status = status;
    }

    #[inline]
    pub fn log0(&mut self, line: u32, fmt: &[u8]) {
        if self.write_log_inner(line, fmt, &[]).is_none() {
            self.status = TestStatus::FrameworkError;
        }
    }

    #[inline]
    pub fn log1(&mut self, line: u32, fmt: &[u8], a0: u64) {
        if self.write_log_inner(line, fmt, &[a0]).is_none() {
            self.status = TestStatus::FrameworkError;
        }
    }

    #[inline]
    pub fn log2(&mut self, line: u32, fmt: &[u8], a0: u64, a1: u64) {
        if self.write_log_inner(line, fmt, &[a0, a1]).is_none() {
            self.status = TestStatus::FrameworkError;
        }
    }

    #[inline]
    pub fn log3(&mut self, line: u32, fmt: &[u8], a0: u64, a1: u64, a2: u64) {
        if self.write_log_inner(line, fmt, &[a0, a1, a2]).is_none() {
            self.status = TestStatus::FrameworkError;
        }
    }

    #[inline]
    pub fn log(&mut self, line: u32, fmt: &[u8], args: &[u64]) {
        if self.write_log_inner(line, fmt, args).is_none() {
            self.status = TestStatus::FrameworkError;
        }
    }

    #[inline(always)]
    pub fn assert_buffer_inner(
        &mut self,
        file: &'static str,
        line: u32,
        ctx: &TcContext,
        name: &'static str,
        first_layer: &'static str,
        offset: usize,
        buf: &[u8],
        len: usize,
        msg_too_short: &'static str,
        msg_mismatch: &'static str,
    ) {
        let mut data = ctx.data();
        let data_end = ctx.data_end();
        let mut pass = true;

        data += offset;

        if data + len > data_end {
            pass = false;
            self.log3(
                line,
                b"CTX len (%d) - offset (%d) < LEN (%d)",
                ctx.len() as u64,
                offset as u64,
                len as u64,
            );
        }
        if buf.len() < len {
            pass = false;
            self.log2(line, msg_too_short.as_bytes(), buf.len() as u64, len as u64);
        }
        if pass && !memcmp(data as *const u8, buf.as_ptr(), len) {
            pass = false;
            self.log0(line, msg_mismatch.as_bytes());
        }

        if !pass {
            self.add_failure(name, file, line, first_layer, buf, len, data, data_end);
            self.fail();
        }
    }

    #[inline]
    pub fn fail(&mut self) {
        self.status = TestStatus::Fail;
    }

    #[inline]
    pub fn skip(&mut self) {
        self.status = TestStatus::Skip;
    }

    #[inline]
    fn write_log_inner(&mut self, line: u32, fmt: &[u8], args: &[u64]) -> Option<()> {
        self.writer.write_bytes(Tag::LogFmt, fmt)?;
        self.writer.write_u32(Tag::LogLine, line)?;
        for &a in args {
            self.writer.write_u64(Tag::LogArg, a)?;
        }
        Some(())
    }

    #[inline(always)]
    fn add_failure(
        &mut self,
        name: &'static str,
        file: &'static str,
        line: u32,
        first_layer: &'static str,
        buf: &[u8],
        len: usize,
        data: usize,
        data_end: usize,
    ) {
        let entry = match SCAPY_ASSERT_MAP_SCRATCH.get_ptr_mut(0) {
            Some(ptr) => unsafe { &mut *ptr },
            None => return,
        };

        copy_str(&mut entry.name, name);
        copy_str(&mut entry.file, file);
        copy_str(&mut entry.first_layer, first_layer);

        entry.line = line;

        entry.expected_len = len;
        entry.expected_buf.fill(0);
        memcpy(entry.expected_buf.as_mut_ptr(), buf.as_ptr(), len);

        entry.actual_len = data_end - data;
        entry.actual_buf.fill(0);
        if data + len <= data_end {
            memcpy(entry.actual_buf.as_mut_ptr(), data as *const u8, len);
        } else {
            memcpy(
                entry.actual_buf.as_mut_ptr(),
                &SCAPY_ASSERT_NULL.actual_buf as *const u8,
                len,
            );
        }

        let idx = match SCAPY_ASSERT_MAP_COUNT.get_ptr_mut(0) {
            Some(ptr) => unsafe {
                let v = &mut *ptr;
                let cur = *v;
                *v = cur + 1;
                cur
            },
            None => return,
        };

        if SCAPY_ASSERT_MAP.set(idx, entry, 0).is_err() {
            test_log!(self, "ERROR: unable to push failed assert to map!");
        }
    }
}

#[inline]
pub fn memcpy(dst: *mut u8, src: *const u8, len: usize) {
    for i in 0..len {
        unsafe { *dst.add(i) = *src.add(i) };
    }
}

#[inline]
pub fn memcmp(buf1: *const u8, buf2: *const u8, len: usize) -> bool {
    for i in 0..len {
        if unsafe { *buf1.add(i) } != unsafe { *buf2.add(i) } {
            return false;
        }
    }
    true
}

#[inline]
fn copy_str(dst: &mut [u8; SCAPY_MAX_STR_LEN], src: &str) {
    dst.fill(0);

    let bytes = src.as_bytes();
    let len = bytes.len().min(SCAPY_MAX_STR_LEN - 1);

    dst[..len].copy_from_slice(&bytes[..len]);
}
