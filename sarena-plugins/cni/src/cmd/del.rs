use rscni_plugin::{
    error::Error,
    types::{Args, CNIResult},
};
use sarena_api_client::attachment_id;
use sarena_infra::{InfraError, NetlinkNetworkProvisioner, Netns, NetworkProvisioner as _};
use tracing::debug;

use crate::{Res, args::ArgsSpec};

pub(crate) async fn del(args: Args, _cni_args: ArgsSpec) -> Res<CNIResult> {
    let Some(netns_path) = args.netns() else {
        return Err(Error::InvalidNetworkConfig("missing CNI_NETNS".to_string()));
    };
    let Some(ifname) = args.ifname() else {
        return Err(Error::InvalidNetworkConfig(
            "missing CNI_IFNAME".to_string(),
        ));
    };
    let Some(container_id) = args.container_id() else {
        return Err(Error::InvalidNetworkConfig(
            "missing CNI_CONTAINERID".to_string(),
        ));
    };

    let api_client = crate::client::build_api_client(&args)?;

    // We delete the endpoint here best effort. If the daemon is not available, or not responsive,
    // the endpoint is still present at the daemon. It would be better, to queue endpoint deletion,
    // and retry deleting endpoints when the daemon is responsive again.
    api_client
        .endpoint()
        .delete(&attachment_id(container_id, ifname))
        .await
        .map_err(|_| Error::PluginNotAvailable("could not delete endpoint".to_string()))?;

    let netns = Netns::open_path(netns_path).map_err(|e| {
        Error::InvalidNetworkConfig(format!(
            "could not open namespace {}: {e}",
            netns_path.display()
        ))
    })?;

    let network_provisioner = NetlinkNetworkProvisioner;
    match network_provisioner.delete_link_in_ns(&netns, ifname).await {
        Ok(()) => debug!("link deleted"),
        Err(InfraError::LinkNotFound(_)) => debug!("link already deleted delete"),
        Err(e) => debug!("could not delete interface in namespace: {e}"),
    }

    Ok(CNIResult::default())
}
