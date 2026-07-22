#![cfg(test)]

use std::str::Utf8Error;

use aya::{EbpfError, maps::MapError, programs::ProgramError};
use sarena_common_test::tlv_reader::ParseError;

mod ebpf_test_runner;
mod report;

#[derive(Debug, thiserror::Error)]
pub enum TestRunnerError {
    #[error("eBPF program error: {0}")]
    EbpfError(#[from] EbpfError),

    #[error("eBPF program error: {0}")]
    ProgramError(#[from] ProgramError),

    #[error("Program {0} not found")]
    ProgramNotFound(String),

    #[error("eBPF map error: {0}")]
    MapError(#[from] MapError),

    #[error("Map {0} not found")]
    MapNotFound(String),

    #[error("Regex error: {0}")]
    RegexError(#[from] regex::Error),

    #[error("Test '{0}' has no assert program")]
    MissingCheck(String),

    #[error("Test has no result")]
    NoResult,

    #[error("Parse failed: {0}")]
    ParseError(#[from] ParseError),

    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] Utf8Error),

    #[error("JSON serialization error: {0}")]
    SerdeError(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Res<T> = Result<T, TestRunnerError>;
