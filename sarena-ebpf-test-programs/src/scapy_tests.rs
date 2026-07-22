use aya_ebpf::programs::TcContext;
use sarena_common_test::{SCAPY_ASSERT_NULL, TestStatus};
use sarena_ebpf_common::PacketBuilder;
use sarena_ebpf_test_framework::{
    TestSuite, assert_buffer, assert_test, status,
    suite::{SCAPY_ASSERT_MAP, SCAPY_ASSERT_MAP_COUNT, memcmp},
};
use sarena_test_macros::{arrange, assert};

#[arrange(tc, "1_basic_test_equal_bytes")]
pub fn basic_test_equal_bytes_arrange(ctx: TcContext) -> TestStatus {
    let mut builder = PacketBuilder::new(&ctx);
    builder.push_data(&scapy_bytes::SCAPY_SST_REQ_BYTES);
    builder.build();
    TestStatus::Pass
}

#[assert(tc, "1_basic_test_equal_bytes")]
pub fn basic_test_equal_bytes_assert(ctx: TcContext, t: &mut TestSuite) {
    assert_buffer!(
        t,
        &ctx,
        "equal bytes",
        "Ether",
        0,
        &scapy_bytes::SCAPY_SST_REQ_BYTES,
        scapy_bytes::SCAPY_SST_REQ_BYTES.len()
    );
}

#[arrange(tc, "2_basic_test_non_equal_bytes")]
pub fn basic_test_non_equal_bytes_arrange(ctx: TcContext) -> TestStatus {
    let mut builder = PacketBuilder::new(&ctx);
    builder.push_data(&scapy_bytes::SCAPY_SST_REQ_BYTES);
    builder.build();
    TestStatus::Pass
}

#[assert(tc, "2_basic_test_non_equal_bytes")]
pub fn basic_test_non_equal_bytes_assert(ctx: TcContext, t: &mut TestSuite) {
    assert_buffer!(
        t,
        &ctx,
        "non-equal bytes",
        "Ether",
        0,
        &scapy_bytes::SCAPY_SST_REP_BYTES,
        scapy_bytes::SCAPY_SST_REP_BYTES.len()
    );
    // Mark the test as "Pass" because this is expected.
    status!(t, TestStatus::Pass);
    let count = SCAPY_ASSERT_MAP_COUNT.get(0).expect("get assert map count");
    assert_test!(t, count == &1);

    let entry = SCAPY_ASSERT_MAP.get(0).expect("get assert map");
    let equal = memcmp(
        entry.expected_buf.as_ptr(),
        scapy_bytes::SCAPY_SST_REP_BYTES.as_ptr(),
        scapy_bytes::SCAPY_SST_REP_BYTES.len(),
    );
    assert_test!(t, equal);
    let equal = memcmp(
        entry.actual_buf.as_ptr(),
        scapy_bytes::SCAPY_SST_REQ_BYTES.as_ptr(),
        scapy_bytes::SCAPY_SST_REQ_BYTES.len(),
    );
    assert_test!(t, equal);
    assert_test!(
        t,
        memcmp(
            entry.name.as_ptr(),
            "non-equal bytes".as_ptr(),
            "non-equal bytes".len()
        )
    );
    assert_test!(
        t,
        scapy_bytes::SCAPY_SST_REP_BYTES.len() == entry.expected_len
    );
    assert_test!(
        t,
        scapy_bytes::SCAPY_SST_REQ_BYTES.len() == entry.actual_len
    );
    assert_test!(t, entry.expected_len == entry.actual_len);

    // Prevent packet compare
    SCAPY_ASSERT_MAP_COUNT
        .set(0, 0, 0)
        .expect("set assert map count");
}

#[arrange(tc, "3_basic_test_too_short")]
pub fn basic_test_too_short_arrange(ctx: TcContext) -> TestStatus {
    let mut builder = PacketBuilder::new(&ctx);
    builder.push_data(&scapy_bytes::SCAPY_SST_REQ_BYTES);
    builder.build();
    TestStatus::Pass
}

#[assert(tc, "3_basic_test_too_short")]
pub fn basic_test_too_short_assert(ctx: TcContext, t: &mut TestSuite) {
    assert_buffer!(
        t,
        &ctx,
        "too short",
        "Ether",
        0,
        &scapy_bytes::SCAPY_SST_REP_PAD_BYTES,
        scapy_bytes::SCAPY_SST_REP_PAD_BYTES.len()
    );
    // Mark the test as "Pass" because this is expected.
    status!(t, TestStatus::Pass);
    let count = SCAPY_ASSERT_MAP_COUNT.get(0).expect("get assert map count");
    assert_test!(t, count == &1);

    let entry = SCAPY_ASSERT_MAP.get(0).expect("get assert map");
    let equal = memcmp(
        entry.expected_buf.as_ptr(),
        scapy_bytes::SCAPY_SST_REP_PAD_BYTES.as_ptr(),
        scapy_bytes::SCAPY_SST_REP_PAD_BYTES.len(),
    );
    assert_test!(t, equal);
    let equal = memcmp(
        entry.actual_buf.as_ptr(),
        SCAPY_ASSERT_NULL.actual_buf.as_ptr(),
        SCAPY_ASSERT_NULL.actual_buf.len(),
    );
    assert_test!(t, equal);
    assert_test!(
        t,
        memcmp(entry.name.as_ptr(), "too short".as_ptr(), "too short".len())
    );
    assert_test!(
        t,
        scapy_bytes::SCAPY_SST_REP_PAD_BYTES.len() == entry.expected_len
    );
    assert_test!(
        t,
        scapy_bytes::SCAPY_SST_REQ_BYTES.len() == entry.actual_len
    );

    // Prevent packet compare
    SCAPY_ASSERT_MAP_COUNT
        .set(0, 0, 0)
        .expect("set assert map count");
}

#[arrange(tc, "4_large_packets")]
pub fn large_packets_arrange(ctx: TcContext) -> TestStatus {
    let mut builder = PacketBuilder::new(&ctx);
    builder.push_data(&scapy_bytes::SCAPY_SST_LPKT_BYTES);
    builder.build();
    TestStatus::Pass
}

#[assert(tc, "4_large_packets")]
pub fn large_packets_assert(ctx: TcContext, t: &mut TestSuite) {
    assert_buffer!(
        t,
        &ctx,
        "large packets",
        "Ether",
        0,
        &scapy_bytes::SCAPY_SST_LPKT_BYTES,
        scapy_bytes::SCAPY_SST_LPKT_BYTES.len()
    );
}

#[arrange(tc, "5_xlarge_packets")]
pub fn xlarge_packets_arrange(ctx: TcContext) -> TestStatus {
    let mut builder = PacketBuilder::new(&ctx);
    builder.push_data(&scapy_bytes::SCAPY_SST_XLPKT_BYTES);
    builder.build();
    TestStatus::Pass
}

#[assert(tc, "5_xlarge_packets")]
pub fn xlarge_packets_assert(ctx: TcContext, t: &mut TestSuite) {
    assert_buffer!(
        t,
        &ctx,
        "xlarge packets",
        "Ether",
        0,
        &scapy_bytes::SCAPY_SST_XLPKT_BYTES,
        scapy_bytes::SCAPY_SST_XLPKT_BYTES.len()
    );
}
