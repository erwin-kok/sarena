pub mod logging;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

impl std::str::FromStr for LogFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(LogFormat::Text),
            "json" => Ok(LogFormat::Json),
            other => Err(format!(
                "invalid log format {other:?}, expected \"text\" or \"json\""
            )),
        }
    }
}

#[derive(Debug, Default)]
pub struct LoggingConfig {
    pub enable_debug: bool,
    pub log_file: Option<String>,
    pub format: LogFormat,
}

pub use logging::{init_logging, shutdown_logging};
