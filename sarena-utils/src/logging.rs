use std::{path::Path, sync::OnceLock};

use tracing::Level;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

use crate::LoggingConfig;

static LOG_INIT: OnceLock<()> = OnceLock::new();
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn init_logging(config: &LoggingConfig) {
    LOG_INIT.get_or_init(|| {
        let level = if config.enable_debug {
            Level::DEBUG
        } else {
            Level::INFO
        };

        let env_filter = EnvFilter::from_default_env().add_directive(level.into());

        let stderr_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .pretty()
            .with_span_events(FmtSpan::ACTIVE);

        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(stderr_layer);

        if let Some(log_path) = &config.log_file {
            let path = Path::new(log_path);

            let dir = path.parent().unwrap_or(Path::new("."));

            // Daily rotation: app.log -> app.log.YYYY-MM-DD
            let file_appender = rolling::daily(dir, path);
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

            let file_layer = fmt::layer()
                .with_writer(non_blocking)
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_span_events(FmtSpan::NONE);

            subscriber
                .with(file_layer)
                .try_init()
                .expect("failed to initialize tracing");

            LOG_GUARD
                .set(guard)
                .expect("Logging guard already initialized");
        } else {
            let _ = subscriber.try_init();
        }
    });
}
