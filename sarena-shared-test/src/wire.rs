pub const WIRE_VERSION: u8 = 1;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Version = 0x01,

    TestName = 0x10,
    TestFile = 0x11,
    TestStatus = 0x13,

    LogFmt = 0x20,
    LogArg = 0x21,
    LogLine = 0x22,
}

impl Tag {
    #[must_use]
    pub const fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Version),

            0x10 => Some(Self::TestName),
            0x11 => Some(Self::TestFile),
            0x13 => Some(Self::TestStatus),

            0x20 => Some(Self::LogFmt),
            0x21 => Some(Self::LogArg),
            0x22 => Some(Self::LogLine),

            _ => None,
        }
    }
}

#[must_use]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Pass = 101,
    Fail = 102,
    Skip = 103,
    FrameworkError = 105,
}

impl TestStatus {
    pub const fn from_u8(v: u8) -> Self {
        match v {
            101 => Self::Pass,
            102 => Self::Fail,
            103 => Self::Skip,
            _ => Self::FrameworkError,
        }
    }
}
