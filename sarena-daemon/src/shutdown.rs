//! The daemon's long-running tasks and its graceful shutdown.

use std::{collections::HashMap, future::Future, process, time::Duration};

use anyhow::Context as _;
use sarena_data_plane::DataPlane;
use sarena_utils::logging;
use tokio::{
    signal::unix::{SignalKind, signal},
    task::{Id, JoinError, JoinSet},
    time::Instant,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// How long a single shutdown step may take.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
/// How long flushing telemetry on shutdown may take.
const TELEMETRY_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the whole shutdown may take, across all steps.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(25);

type TaskResult = anyhow::Result<()>;

/// The daemon's long-running tasks, each known by a name for logging.
///
/// Every task is expected to run until the shutdown token is cancelled;
/// one that returns earlier triggers a shutdown, see [`wait_for_shutdown`].
#[derive(Default)]
pub struct DaemonTasks {
    tasks: JoinSet<TaskResult>,
    names: HashMap<Id, &'static str>,
}

impl DaemonTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawns `task` under `name`.
    pub fn spawn<F>(&mut self, name: &'static str, task: F)
    where
        F: Future<Output = TaskResult> + Send + 'static,
    {
        let handle = self.tasks.spawn(task);
        self.names.insert(handle.id(), name);
    }

    /// Waits for the next task to finish and logs how it ended. `None` when
    /// no tasks are left.
    async fn join_next(&mut self) -> Option<()> {
        let result = self.tasks.join_next_with_id().await?;
        self.log_result(result);
        Some(())
    }

    /// Logs a finished task under its name, removing it from `names` so that
    /// what is left there are the tasks still running.
    fn log_result(&mut self, result: Result<(Id, TaskResult), JoinError>) {
        let id = match &result {
            Ok((id, _)) => *id,
            Err(err) => err.id(),
        };
        let task = self.names.remove(&id).unwrap_or("unknown");
        match result {
            Ok((_, Ok(()))) => info!(task, "daemon task exited"),
            Ok((_, Err(err))) => error!(task, ?err, "daemon task failed"),
            Err(err) => error!(task, ?err, "daemon task panicked"),
        }
    }
}

/// Cancels `shutdown` on SIGINT or SIGTERM; a second signal exits the
/// process immediately.
pub fn spawn_signal_handler(shutdown: CancellationToken) -> anyhow::Result<()> {
    let mut sigint = signal(SignalKind::interrupt()).context("failed to install SIGINT handler")?;
    let mut sigterm =
        signal(SignalKind::terminate()).context("failed to install SIGTERM handler")?;

    tokio::spawn(async move {
        tokio::select! {
            _ = sigint.recv() => info!("received SIGINT, shutting down"),
            _ = sigterm.recv() => info!("received SIGTERM, shutting down"),
        }
        shutdown.cancel();

        let kind = tokio::select! {
            _ = sigint.recv() => SignalKind::interrupt(),
            _ = sigterm.recv() => SignalKind::terminate(),
        };
        error!("received second signal, forcing exit");
        process::exit(128 + kind.as_raw_value());
    });

    Ok(())
}

/// Runs until `shutdown` is cancelled (e.g. by a signal) or one of `tasks`
/// exits on its own, then shuts everything down within [`SHUTDOWN_BUDGET`]:
/// the tasks, the data plane and finally telemetry.
///
/// Fails when a task exiting early is what triggered the shutdown.
pub async fn wait_for_shutdown(
    mut tasks: DaemonTasks,
    shutdown: CancellationToken,
    data_plane: impl DataPlane,
) -> anyhow::Result<()> {
    // Wait for either an OS shutdown signal or a task exiting unexpectedly
    // (e.g. panicking, or returning before we asked it to stop). Whichever
    // happens first triggers the same graceful shutdown path below.
    // `biased` so shutdown wins when both branches are ready: tasks return as
    // soon as their child token is cancelled, which must not count as an early exit.
    let early_exit = tokio::select! {
        biased;

        () = shutdown.cancelled() => {
            info!("shutdown signal received");
            false
        }

        Some(()) = tasks.join_next() => {
            error!("daemon task exited before shutdown was requested");
            true
        }
    };

    // No-op on the signal path; on the early-exit path this tells the
    // remaining tasks to stop gracefully.
    shutdown.cancel();

    let deadline = Instant::now() + SHUTDOWN_BUDGET;

    shutdown_step("daemon tasks", SHUTDOWN_TIMEOUT, deadline, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;

    if !tasks.tasks.is_empty() {
        let remaining: Vec<_> = tasks.names.values().collect();
        warn!(
            ?remaining,
            "aborting daemon tasks that did not stop in time"
        );
        tasks.tasks.abort_all();
    }

    shutdown_step("data plane", SHUTDOWN_TIMEOUT, deadline, async {
        if let Err(err) = data_plane.shutdown().await {
            error!(?err, "failed to shutdown data plane");
        }
    })
    .await;

    shutdown_step(
        "tracing",
        TELEMETRY_SHUTDOWN_TIMEOUT,
        deadline,
        shutdown_blocking("tracing", logging::shutdown_tracing),
    )
    .await;

    if early_exit {
        anyhow::bail!("daemon task exited before shutdown was requested");
    }

    Ok(())
}

/// Runs one shutdown step, bounded by its own `timeout` and by the overall
/// shutdown `deadline`, whichever comes first.
async fn shutdown_step(
    name: &str,
    timeout: Duration,
    deadline: Instant,
    fut: impl Future<Output = ()>,
) {
    let step_deadline = deadline.min(Instant::now() + timeout);
    if tokio::time::timeout_at(step_deadline, fut).await.is_err() {
        error!("timed out shutting down {name}");
    }
}

async fn shutdown_blocking(name: &str, f: impl FnOnce() + Send + 'static) {
    if tokio::task::spawn_blocking(f).await.is_err() {
        error!("{name} shutdown task panicked");
    }
}
