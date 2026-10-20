use kube::Client;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    controllers::{address_pool, service},
    error::{KubernetesError, Res},
    webhook,
};

pub async fn start(shutdown: CancellationToken) -> Res<()> {
    let client = Client::try_default().await?;
    let mut tasks = JoinSet::new();

    tracing::info!("starting Kubernetes reconcilers");

    tasks.spawn(address_pool::run(client.clone(), shutdown.child_token()));
    tasks.spawn(service::run(client.clone(), shutdown.child_token()));
    tasks.spawn(webhook::run(client.clone(), shutdown.child_token()));

    // `biased` so shutdown wins when both branches are ready: the controllers
    // hold child tokens and return as soon as `shutdown` is cancelled, so a
    // random pick could misreport that as an unexpected exit.
    tokio::select! {
        biased;

        () = shutdown.cancelled() => {
            tracing::info!("controllers shutting down");
        }

        result = tasks.join_next() => {
            match result {
                Some(Ok(())) => {
                    return Err(KubernetesError::ControllerExited);
                }

                Some(Err(err)) => {
                    return Err(err.into());
                }

                None => {
                    return Err(KubernetesError::AllControllersExited);
                }
            }
        }
    }

    // Let the controllers (and the webhook's graceful shutdown) finish instead
    // of aborting them by dropping the JoinSet. The caller bounds this wait.
    while let Some(result) = tasks.join_next().await {
        if let Err(err) = result {
            tracing::error!(?err, "controller task panicked during shutdown");
        }
    }

    Ok(())
}
