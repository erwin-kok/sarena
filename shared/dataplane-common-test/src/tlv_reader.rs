use std::fmt::{Display, Formatter};

use crate::{Tag, TestStatus, WIRE_VERSION};

#[derive(Debug, Default)]
pub struct LogEntry {
    pub fmt: String,
    pub line: u32,
    pub args: Vec<u64>,
}

#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub file: String,
    pub status: TestStatus,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug)]
pub enum ParseError {
    MissingVersionTag,
    IncompatibleVersion {
        found: u8,
    },
    TruncatedHeader {
        offset: usize,
    },
    TruncatedValue {
        offset: usize,
        expected: usize,
    },
    BadTagLength {
        tag: u8,
        expected: usize,
        found: usize,
    },
    OrphanLogField {
        offset: usize,
    },
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingVersionTag => write!(f, "first record is not a Version tag"),
            Self::IncompatibleVersion { found } => write!(
                f,
                "incompatible wire version: found {found}, expected {WIRE_VERSION}"
            ),
            Self::TruncatedHeader { offset } => {
                write!(f, "truncated TLV header at offset {offset}")
            }
            Self::TruncatedValue { offset, expected } => write!(
                f,
                "truncated TLV value at offset {offset}: need {expected} bytes"
            ),
            Self::BadTagLength {
                tag,
                expected,
                found,
            } => write!(f, "tag {tag:#04x} expects {expected} bytes, found {found}"),
            Self::OrphanLogField { offset } => {
                write!(
                    f,
                    "LogLine/LogArg at offset {offset} with no preceding LogFmt"
                )
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses the single flat test result written by a `TestSuite`/`TEST!` block.
pub fn parse_test(data: &[u8]) -> Result<TestResult, ParseError> {
    let mut cur = 0usize;
    let mut name = String::new();
    let mut file = String::new();
    let mut status = TestStatus::FrameworkError;
    let mut logs: Vec<LogEntry> = Vec::new();

    // Version record must come first
    let (tag, value) = next_tlv(data, &mut cur)?;
    if tag != Tag::Version as u8 {
        return Err(ParseError::MissingVersionTag);
    }
    if value != [WIRE_VERSION] {
        return Err(ParseError::IncompatibleVersion { found: value[0] });
    }

    while cur < data.len() {
        let offset = cur;
        let (tag, value) = next_tlv(data, &mut cur)?;
        match Tag::from_u8(tag) {
            Some(Tag::TestName) => {
                name = String::from_utf8_lossy(value).into_owned();
            }
            Some(Tag::TestFile) => {
                file = String::from_utf8_lossy(value).into_owned();
            }
            Some(Tag::TestStatus) => {
                if value.len() != 1 {
                    return Err(ParseError::BadTagLength {
                        tag: Tag::TestStatus as u8,
                        expected: 1,
                        found: value.len(),
                    });
                }
                status = TestStatus::from_u8(value[0]);
            }
            Some(Tag::LogFmt) => {
                logs.push(LogEntry {
                    fmt: String::from_utf8_lossy(value).into_owned(),
                    line: 0,
                    args: Vec::new(),
                });
            }
            Some(Tag::LogLine) => {
                if value.len() != 4 {
                    return Err(ParseError::BadTagLength {
                        tag: Tag::LogLine as u8,
                        expected: 4,
                        found: value.len(),
                    });
                }
                let bytes: [u8; 4] = value.try_into().unwrap();
                let Some(entry) = logs.last_mut() else {
                    return Err(ParseError::OrphanLogField { offset });
                };
                entry.line = u32::from_le_bytes(bytes);
            }
            Some(Tag::LogArg) => {
                if value.len() != 8 {
                    return Err(ParseError::BadTagLength {
                        tag: Tag::LogArg as u8,
                        expected: 8,
                        found: value.len(),
                    });
                }
                let bytes: [u8; 8] = value.try_into().unwrap();
                let Some(entry) = logs.last_mut() else {
                    return Err(ParseError::OrphanLogField { offset });
                };
                entry.args.push(u64::from_le_bytes(bytes));
            }
            _ => { /* unknown tag — skip */ }
        }
    }

    Ok(TestResult {
        name,
        file,
        status,
        logs,
    })
}

fn next_tlv<'a>(data: &'a [u8], cur: &mut usize) -> Result<(u8, &'a [u8]), ParseError> {
    if *cur + 2 > data.len() {
        return Err(ParseError::TruncatedHeader { offset: *cur });
    }
    let tag = data[*cur];
    let len = data[*cur + 1] as usize;
    *cur += 2;
    if *cur + len > data.len() {
        return Err(ParseError::TruncatedValue {
            offset: *cur,
            expected: len,
        });
    }
    let value = &data[*cur..*cur + len];
    *cur += len;
    Ok((tag, value))
}
