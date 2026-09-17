use std::{convert::Infallible, sync::Arc, time::Duration};

use futures::stream::StreamExt;
use kube::{
    Api, Client, ResourceExt,
    runtime::{Controller, controller::Action, watcher},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::crd::address_pool::AddressPool;

struct Context {
    _client: Client,
}

pub async fn run(client: Client, shutdown: CancellationToken) {
    let api: Api<AddressPool> = Api::all(client.clone());

    let context = Arc::new(Context {
        _client: client.clone(),
    });

    info!("starting AddressPool controller");

    Controller::new(api, watcher::Config::default())
        .run(reconcile, error_policy, context)
        .take_until(shutdown.cancelled())
        .for_each(|result| async move {
            match result {
                Ok((obj, action)) => {
                    info!(
                        resource = %obj.name,
                        ?action,
                        "reconciliation complete"
                    );
                }

                Err(err) => {
                    error!(?err, "reconciliation failed");
                }
            }
        })
        .await;

    info!("AddressPool controller stopped");
}

async fn reconcile(_resource: Arc<AddressPool>, _ctx: Arc<Context>) -> Result<Action, Infallible> {


    Ok(Action::requeue(Duration::from_secs(30)))
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn error_policy(resource: Arc<AddressPool>, error: &Infallible, _ctx: Arc<Context>) -> Action {
    tracing::error!(
        resource = %resource.name_any(),
        ?error,
        "reconciliation error"
    );
    Action::requeue(Duration::from_secs(5))
}
