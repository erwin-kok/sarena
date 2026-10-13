use core::fmt;

use serde::{Deserialize, Serialize};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_HASH: &str = env!("SARENA_CLI_GIT_HASH");
pub const BUILD_DATE: &str = env!("SARENA_CLI_BUILD_DATE");

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct SarenaVersion {
    pub version: String,
    pub git_hash: String,
    pub build_date: String,
    pub os: String,
    pub arch: String,
}

impl fmt::Display for SarenaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, built {}, {}/{})",
            self.version, self.git_hash, self.build_date, self.os, self.arch
        )
    }
}
