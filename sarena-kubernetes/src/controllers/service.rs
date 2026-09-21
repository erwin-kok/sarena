use std::{convert::Infallible, sync::Arc, time::Duration};

use futures::stream::StreamExt;
use k8s_openapi::api::core::v1::Service;
use kube::{
    Api, Client, ResourceExt,
    runtime::{Controller, controller::Action, watcher},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

struct Context {
    _client: Client,
}

pub async fn run(client: Client, shutdown: CancellationToken) {
    let api: Api<Service> = Api::all(client.clone());

    let context = Arc::new(Context {
        _client: client.clone(),
    });

    debug!("starting Service controller");

    Controller::new(api, watcher::Config::default())
        .run(reconciler, error_policy, context)
        .take_until(shutdown.cancelled())
        .for_each(|result| async move {
            match result {
                Ok((obj, action)) => {
                    debug!(
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

    debug!("service controller stopped");
}

async fn reconciler(_resource: Arc<Service>, _ctx: Arc<Context>) -> Result<Action, Infallible> {
    info!("HERE");

    Ok(Action::requeue(Duration::from_secs(30)))
}

fn error_policy(resource: Arc<Service>, error: &Infallible, _ctx: Arc<Context>) -> Action {
    warn!(
        resource = %resource.name_any(),
        ?error,
        "reconciliation error"
    );
    Action::requeue(Duration::from_secs(5))
}
