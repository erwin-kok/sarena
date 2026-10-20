use metrics::{describe_histogram, histogram};
use sarena_utils::spans::SpanStat;

const BOOTSTRAP_HISTOGRAM: &str = "agent_bootstrap_seconds";

#[derive(Default)]
pub struct BootstrapStatistics {
    pub overall: SpanStat,
}

impl BootstrapStatistics {
    pub fn new() -> Self {
        describe_histogram!(BOOTSTRAP_HISTOGRAM, "Duration of bootstrap sequence");
        Self::default()
    }

    fn named_stats(&self) -> [(&'static str, &SpanStat); 1] {
        [("overall", &self.overall)]
    }

    fn record_duration(scope: &'static str, outcome: &'static str, duration: std::time::Duration) {
        if duration.is_zero() {
            return;
        }

        histogram!(
            BOOTSTRAP_HISTOGRAM,
            "scope" => scope,
            "outcome" => outcome,
        )
        .record(duration.as_secs_f64());
    }

    pub fn update_metrics(&self) {
        for (scope, stat) in self.named_stats() {
            Self::record_duration(scope, "success", stat.success_total());
            Self::record_duration(scope, "failure", stat.failure_total());
        }
    }
}
