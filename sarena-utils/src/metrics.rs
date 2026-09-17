use anyhow::Result;
use opentelemetry::global;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::Registry;

pub struct MetricsState {
    pub registry: Registry,
    pub provider: SdkMeterProvider,
}

pub fn init_metrics() -> Result<MetricsState> {
    let registry = Registry::default();
    let exporter = opentelemetry_prometheus::exporter()
        .with_registry(registry.clone())
        .build()?;
    let provider = SdkMeterProvider::builder().with_reader(exporter).build();
    global::set_meter_provider(provider.clone());
    Ok(MetricsState { registry, provider })
}
