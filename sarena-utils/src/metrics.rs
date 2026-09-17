use anyhow::Result;
use opentelemetry::{KeyValue, global};
use opentelemetry_sdk::{Resource, metrics::SdkMeterProvider};
use opentelemetry_semantic_conventions::resource;
use prometheus::Registry;

use crate::version;

pub struct MetricsState {
    pub registry: Registry,
    pub provider: SdkMeterProvider,
}

pub fn init_metrics() -> Result<MetricsState> {
    let registry = Registry::default();
    let exporter = opentelemetry_prometheus::exporter()
        .with_registry(registry.clone())
        .build()?;
    let resource = Resource::builder()
        .with_attribute(KeyValue::new(resource::SERVICE_NAME, "sarena-daemon"))
        .with_attribute(KeyValue::new(resource::SERVICE_VERSION, version::VERSION))
        .build();
    let provider = SdkMeterProvider::builder()
        .with_reader(exporter)
        .with_resource(resource)
        .build();
    global::set_meter_provider(provider.clone());
    Ok(MetricsState { registry, provider })
}
