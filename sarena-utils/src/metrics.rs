use opentelemetry::global;
use opentelemetry_prometheus;
use opentelemetry_sdk::metrics::SdkMeterProvider;

fn init_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let exporter = opentelemetry_prometheus::exporter().build()?;

    let provider = SdkMeterProvider::builder().with_reader(exporter).build();

    global::set_meter_provider(provider);

    Ok(())
}
