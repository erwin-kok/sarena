pub mod logging;

#[derive(Debug)]
pub struct LoggingConfig {
    pub enable_debug: bool,
    pub log_file: Option<String>,
}

pub use logging::{init_logging, shutdown_logging};
