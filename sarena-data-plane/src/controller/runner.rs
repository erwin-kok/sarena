use std::{sync::Arc, time::Duration};

use tokio::{sync::watch, time::Instant};
use tokio_util::sync::CancellationToken;

use crate::controller::{
    guard::{RunError, catch_panic, with_timeout},
    job::Flow,
    spec::Spec,
    status::{ControllerObserver, StatusCell},
};

/// How long the runner waits before running the job again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// Run again after this delay, or sooner on an update.
    For(Duration),
    /// Run again only on an update.
    UntilUpdate,
}

impl Wait {
    /// `None` means there is no delay to wait for, so wait for an update.
    fn from_delay(delay: Option<Duration>) -> Self {
        match delay {
            Some(d) => Wait::For(d),
            None => Wait::UntilUpdate,
        }
    }

    /// Resolves when the wait is over; never resolves for `UntilUpdate`.
    async fn elapsed(self) {
        match self {
            Wait::For(d) => tokio::time::sleep(d).await,
            Wait::UntilUpdate => std::future::pending().await,
        }
    }
}

/// What ended a wait.
enum Wake {
    /// The delay passed.
    Timer,
    /// A new spec arrived; run it at once.
    SpecChanged(Spec),
    /// The controller was removed, or its manager dropped.
    Stopped,
}

/// The state owned by one controller task.
pub(crate) struct Runner {
    name: String,
    spec_rx: watch::Receiver<Spec>,
    cancel: CancellationToken,
    status: Arc<StatusCell>,
    observer: Option<Arc<dyn ControllerObserver>>,
    consecutive_errors: u32,
}

impl Runner {
    pub(crate) fn new(
        name: String,
        spec_rx: watch::Receiver<Spec>,
        cancel: CancellationToken,
        status: Arc<StatusCell>,
        observer: Option<Arc<dyn ControllerObserver>>,
    ) -> Self {
        Self {
            name,
            spec_rx,
            cancel,
            status,
            observer,
            consecutive_errors: 0,
        }
    }

    /// Runs the job until the controller is stopped, then runs its stop hook.
    pub(crate) async fn run(mut self) {
        let mut spec = self.spec_rx.borrow_and_update().clone();

        loop {
            let started = Instant::now();
            let result = self.run_job(&spec).await;
            self.record(&result, started.elapsed());

            let wait = self.next_wait(&spec, &result);
            match self.wait(wait).await {
                Wake::Timer => {}
                Wake::SpecChanged(new) => spec = new,
                Wake::Stopped => break,
            }
        }

        tracing::debug!(controller = %self.name, "controller stopping");
        self.run_stop_hook(&spec).await;
    }

    async fn run_job(&self, spec: &Spec) -> Result<Flow, RunError> {
        let job = spec.job.clone();
        let token = self.cancel.clone();
        // The call happens inside the async block, so a panic while building
        // the future is caught as well.
        catch_panic(async move { job(token).await }).await
    }

    /// Updates the status, the observer and the consecutive error count.
    fn record(&mut self, result: &Result<Flow, RunError>, elapsed: Duration) {
        self.status.record_run(result, elapsed);
        if let Some(observer) = &self.observer {
            observer.run_finished(&self.name, result.is_ok(), elapsed);
        }

        match result {
            Ok(_) => self.consecutive_errors = 0,
            Err(_) => self.consecutive_errors = self.consecutive_errors.saturating_add(1),
        }
    }

    fn next_wait(&self, spec: &Spec, result: &Result<Flow, RunError>) -> Wait {
        match result {
            Ok(Flow::Continue) => Wait::from_delay(spec.interval),
            Ok(Flow::Done) => Wait::UntilUpdate,
            Err(e) => {
                let retry_in = spec
                    .backoff
                    .jittered(self.consecutive_errors, rand::random())
                    .or(spec.interval);

                tracing::debug!(
                    controller = %self.name,
                    error = %e,
                    consecutive = self.consecutive_errors,
                    ?retry_in,
                    "controller run failed"
                );

                Wait::from_delay(retry_in)
            }
        }
    }

    /// Waits for `wait` to pass, an update, or a stop; stop wins ties.
    async fn wait(&mut self, wait: Wait) -> Wake {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Wake::Stopped,
            changed = self.spec_rx.changed() => match changed {
                Ok(()) => Wake::SpecChanged(self.spec_rx.borrow_and_update().clone()),
                // The manager dropped the sender.
                Err(_) => Wake::Stopped,
            },
            () = wait.elapsed() => Wake::Timer,
        }
    }

    async fn run_stop_hook(&self, spec: &Spec) {
        let Some(on_stop) = spec.on_stop.clone() else {
            return;
        };

        if let Err(e) = with_timeout(spec.stop_timeout, async move { on_stop().await }).await {
            self.status.record_stop_error(&e);
        }
    }
}
