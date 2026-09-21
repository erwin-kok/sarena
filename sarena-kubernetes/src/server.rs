use std::sync::Arc;

use kube::Client;
use prometheus::Registry;
use tokio::{sync::Mutex, task::JoinSet};
use tokio_util::sync::CancellationToken;

use crate::{
    controllers::{address_pool, service},
    error::{KubernetesError, Res},
    metrics::Metrics,
};

pub async fn start(metrics_registry: Registry, shutdown: CancellationToken) -> Res<()> {
    let client = Client::try_default().await?;
    let mut tasks = JoinSet::new();

    tracing::info!("starting Kubernetes reconcilers");

    let metrics = Arc::new(Mutex::new(
        Metrics::default().register(&metrics_registry).unwrap(),
    ));

    tasks.spawn(address_pool::run(client.clone(), shutdown.child_token()));
    tasks.spawn(service::run(client.clone(), shutdown.child_token()));

    tokio::select! {
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

    Ok(())
}
