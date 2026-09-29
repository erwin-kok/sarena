use prometheus::Registry;

#[derive(Debug, Clone, Default)]
pub struct Metrics {}

impl Metrics {
    pub fn register(self, _registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(self)
    }
}
