use rscni_plugin::{error::Error, types::Args};
use sarena_api_client::ApiClient;

use crate::{Res, args::ArgsSpec};

pub(crate) async fn status(_args: Args, _cni_args: ArgsSpec) -> Res<()> {
    let api_client = ApiClient::new_default_client()
        .map_err(|_| Error::PluginNotAvailable("DaemonDown".to_string()))?;

    api_client
        .daemon()
        .health()
        .await
        .map_err(|_| Error::PluginNotAvailable("could not get daemon health".to_string()))?;

    Ok(())
}
