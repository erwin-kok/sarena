use std::{
    path::Path,
    sync::{Mutex, OnceLock},
};

use opentelemetry::{KeyValue, trace::TracerProvider};
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use opentelemetry_semantic_conventions::resource;
use tracing::Level;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{
    EnvFilter, Layer, Registry,
    fmt::{self, format::FmtSpan},
    layer::{Layered, SubscriberExt},
    util::SubscriberInitExt,
};

use crate::{LogFormat, TracingConfig, version};

static LOG_INIT: OnceLock<()> = OnceLock::new();
static LOG_GUARD: Mutex<Option<LoggingGuard>> = Mutex::new(None);

struct LoggingGuard {
    // Held only to flush buffered logs/spans on drop; never read otherwise.
    #[allow(dead_code)]
    worker_guard: Option<WorkerGuard>,
    tracer_provider: Option<SdkTracerProvider>,
}

pub fn init_tracing(config: &TracingConfig) {
    LOG_INIT.get_or_init(|| {
        let _ = tracing_log::LogTracer::init();

        let level = if config.enable_debug {
            Level::DEBUG
        } else {
            Level::INFO
        };

        let env_filter = EnvFilter::from_default_env().add_directive(level.into());

        let stderr_layer: Box<dyn Layer<Layered<EnvFilter, Registry>> + Send + Sync> =
            match config.format {
                LogFormat::Text => fmt::layer()
                    .with_ansi(true)
                    .with_writer(std::io::stderr)
                    .compact()
                    .with_span_events(FmtSpan::CLOSE)
                    .boxed(),
                LogFormat::Json => fmt::layer()
                    .with_writer(std::io::stderr)
                    .json()
                    .with_current_span(true)
                    .with_span_list(true)
                    .with_span_events(FmtSpan::NONE)
                    .boxed(),
            };

        let mut tracer_provider = None;
        let otel_layer = config.otel_endpoint.as_ref().map(|otel_endpoint| {
            let resource = Resource::builder()
                .with_attribute(KeyValue::new(resource::SERVICE_NAME, "sarena-daemon"))
                .with_attribute(KeyValue::new(resource::SERVICE_VERSION, version::VERSION))
                .build();
            let exporter = SpanExporter::builder()
                .with_tonic()
                .with_endpoint(otel_endpoint)
                .build()
                .expect("failed to initialize SpanExporter");
            let provider = SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource)
                .build();
            let tracer = provider.tracer("sarena-daemon");
            let layer = tracing_opentelemetry::layer().with_tracer(tracer);

            tracer_provider = Some(provider);

            layer
        });

        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(stderr_layer)
            .with(otel_layer);

        let worker_guard = if let Some(log_path) = &config.log_file {
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

            Some(guard)
        } else {
            let _ = subscriber.try_init();
            None
        };

        *LOG_GUARD.lock().expect("logging guard mutex poisoned") = Some(LoggingGuard {
            worker_guard,
            tracer_provider,
        });
    });
}

pub fn shutdown_tracing() {
    let Some(guard) = LOG_GUARD
        .lock()
        .expect("logging guard mutex poisoned")
        .take()
    else {
        return;
    };

    if let Some(provider) = &guard.tracer_provider
        && let Err(err) = provider.shutdown()
    {
        tracing::error!(?err, "failed to shut down OTel tracer provider");
    }
}
