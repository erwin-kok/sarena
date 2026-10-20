use std::{
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct SpanStat {
    inner: Mutex<SpanStatInner>,
}

#[derive(Default)]
struct SpanStatInner {
    start: Option<Instant>,
    success_duration: Duration,
    failure_duration: Duration,
}

impl SpanStatInner {
    fn end_span(&mut self, success: bool) {
        if let Some(start) = self.start.take() {
            let duration = start.elapsed();
            if success {
                self.success_duration += duration;
            } else {
                self.failure_duration += duration;
            }
        }
    }
}

impl SpanStat {
    /// Start a new span.
    ///
    /// # Panics
    ///
    /// If a span is already running.
    pub fn start(&self) {
        let mut inner = self.inner();
        assert!(inner.start.is_none(), "span already started");
        inner.start = Some(Instant::now());
    }

    /// End the running span, adding its duration to the success or failure total. No-op if no
    /// span is running.
    pub fn end(&self, success: bool) {
        self.inner().end_span(success);
    }

    /// Total duration of all spans. A running span is ended first, as a success.
    pub fn total(&self) -> Duration {
        let mut inner = self.inner();
        inner.end_span(true);
        inner.success_duration + inner.failure_duration
    }

    pub fn success_total(&self) -> Duration {
        self.inner().success_duration
    }

    pub fn failure_total(&self) -> Duration {
        self.inner().failure_duration
    }

    /// Clear both totals and any running span.
    pub fn reset(&self) {
        *self.inner() = SpanStatInner::default();
    }

    pub fn seconds(&self) -> u64 {
        self.total().as_secs()
    }

    fn inner(&self) -> MutexGuard<'_, SpanStatInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;

    use super::*;

    const SPAN: Duration = Duration::from_millis(5);

    #[test]
    fn end_splits_success_and_failure() {
        let stat = SpanStat::default();

        stat.start();
        sleep(SPAN);
        stat.end(true);
        stat.start();
        sleep(SPAN);
        stat.end(false);

        assert!(stat.success_total() >= SPAN);
        assert!(stat.failure_total() >= SPAN);
        assert_eq!(stat.total(), stat.success_total() + stat.failure_total());
    }

    #[test]
    fn total_ends_running_span_as_success() {
        let stat = SpanStat::default();

        stat.start();
        sleep(SPAN);
        let total = stat.total();

        assert!(total >= SPAN);
        assert_eq!(stat.success_total(), total);
        // The span was ended, so a new one can start.
        stat.start();
    }

    #[test]
    fn end_without_start_is_noop() {
        let stat = SpanStat::default();
        stat.end(false);
        assert_eq!(stat.total(), Duration::ZERO);
    }

    #[test]
    fn reset_clears_totals_and_running_span() {
        let stat = SpanStat::default();
        stat.start();
        sleep(SPAN);
        stat.end(true);
        stat.start();

        stat.reset();

        assert_eq!(stat.total(), Duration::ZERO);
        stat.start();
    }

    #[test]
    #[should_panic(expected = "span already started")]
    fn double_start_panics() {
        let stat = SpanStat::default();
        stat.start();
        stat.start();
    }
}
