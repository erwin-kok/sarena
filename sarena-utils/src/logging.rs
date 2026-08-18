use std::{
    path::Path,
    sync::{Mutex, OnceLock},
};

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
static LOG_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

pub fn init_logging(config: &LoggingConfig) {
    LOG_INIT.get_or_init(|| {
        let _ = tracing_log::LogTracer::init();

        let level = if config.enable_debug {
            Level::DEBUG
        } else {
            Level::INFO
        };

        let env_filter = EnvFilter::from_default_env().add_directive(level.into());

        let stderr_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .pretty()
            .with_span_events(FmtSpan::CLOSE);

        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(stderr_layer);

        if let Some(log_path) = &config.log_file {
            let path = Path::new(log_path);

            let dir = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));

            let file_name_prefix = path.file_name().unwrap_or(path.as_os_str());
            let file_appender = rolling::daily(dir, file_name_prefix);
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

            *LOG_GUARD.lock().expect("logging guard mutex poisoned") = Some(guard);
        } else {
            let _ = subscriber.try_init();
        }
    });
}

pub fn shutdown_logging() {
    LOG_GUARD
        .lock()
        .expect("logging guard mutex poisoned")
        .take();
}
