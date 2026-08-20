use sarena_utils::{LoggingConfig, logging};
use tracing::info;

#[tokio::main]
async fn main() {
    logging::init_logging(&LoggingConfig {
        enable_debug: false,
        log_file: None,
    });

    info!("starting sarena-daemon");

    logging::shutdown_logging();
}
