use std::{
    sync::RwLock,
    time::{Duration, Instant},
};

pub struct SpanStat {
    inner: RwLock<SpanStatInner>,
}

pub struct SpanStatInner {
    start: Instant,
    success_duration: Duration,
    failure_duration: Duration,
}

impl SpanStat {
    #[must_use]
    pub fn start() -> Self {
        Self {
            inner: RwLock::new(SpanStatInner {
                start: Instant::now(),
                success_duration: Duration::ZERO,
                failure_duration: Duration::ZERO,
            }),
        }
    }

    pub fn end(&self, success: bool) {
        let mut inner = self.inner.write().expect("spanstat poisened");
        let d = Instant::now().duration_since(inner.start);
        if success {
            inner.success_duration += d;
        } else {
            inner.failure_duration += d;
        }
    }

    pub fn total(&self) -> Duration {
        let inner = self.inner.write().expect("spanstat poisened");
        inner.success_duration + inner.failure_duration
    }

    pub fn success_total(&self) -> Duration {
        let inner = self.inner.write().expect("spanstat poisened");
        inner.success_duration
    }

    pub fn failure_total(&self) -> Duration {
        let inner = self.inner.write().expect("spanstat poisened");
        inner.failure_duration
    }

    pub fn reset(&self) {
        let mut inner = self.inner.write().expect("spanstat poisened");
        inner.success_duration = Duration::ZERO;
        inner.failure_duration = Duration::ZERO;
    }

    pub fn seconds(&self) -> u64 {
        self.total().as_secs()
    }
}
