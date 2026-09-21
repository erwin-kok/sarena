use prometheus::Registry;

#[derive(Debug, Clone)]
pub struct Metrics {}

impl Default for Metrics {
    fn default() -> Self {
        Self {}
    }
}

impl Metrics {
    pub fn register(self, registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(self)
    }
}
