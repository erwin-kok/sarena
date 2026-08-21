use rscni_plugin::{error::Error, types::Args};

use crate::{Res, args::ArgsSpec};

pub(crate) async fn status(args: Args, _cni_args: ArgsSpec) -> Res<()> {
    let api_client = crate::client::build_api_client(&args)?;

    api_client
        .daemon()
        .health()
        .await
        .map_err(|_| Error::PluginNotAvailable("could not get daemon health".to_string()))?;

    Ok(())
}
