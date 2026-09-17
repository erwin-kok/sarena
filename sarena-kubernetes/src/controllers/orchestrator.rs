use kube::Client;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{
    controllers::address_pool,
    error::{KubernetesError, Res},
};

pub async fn start(shutdown: CancellationToken) -> Res<()> {
    let client = Client::try_default().await?;
    let mut tasks = JoinSet::new();

    tasks.spawn(address_pool::run(client.clone(), shutdown.child_token()));

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
