use std::{collections::HashSet, path::PathBuf};

use rscni_plugin::{
    error::Error,
    types::{Args, CNIResult},
};
use sarena_api_client::{ApiClient, attachment_id};
use sarena_api_types_v1::endpoint;
use sarena_infra::{InterfaceAddress, Link, NetlinkNetworkProvisioner, Netns, NetworkProvisioner};

use crate::{Res, args::ArgsSpec};

pub(crate) async fn check(args: Args, _cni_args: ArgsSpec) -> Res<CNIResult> {
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
    let Some(net_conf) = args.config() else {
        return Err(Error::InvalidNetworkConfig("missing NetConf".to_string()));
    };
    let Some(prev_result) = &net_conf.prev_result else {
        return Err(Error::InvalidNetworkConfig(
            "NetConf does not have a prev_result".to_string(),
        ));
    };

    let api_client = ApiClient::new_default_client()
        .map_err(|_| Error::PluginNotAvailable("DaemonDown".to_string()))?;

    let response = api_client
        .endpoint()
        .health(&attachment_id(container_id, ifname))
        .await
        .map_err(|_| Error::PluginNotAvailable("could not get endpoint health".to_string()))?;

    if response.heatlh == endpoint::EndpointHealthStatus::Failure {
        return Err(Error::Custom(
            100,
            "unhealthy".to_string(),
            "container is unhealthy in agent".to_string(),
        ));
    }

    verify_interface(netns_path, ifname, prev_result).await?;

    Ok(CNIResult::default())
}

async fn verify_interface(
    netns_path: &PathBuf,
    ifname: &str,
    prev_result: &CNIResult,
) -> Res<CNIResult> {
    let netns = Netns::open_path(netns_path).map_err(|e| {
        Error::InvalidNetworkConfig(format!(
            "could not open namespace {}: {e}",
            netns_path.display()
        ))
    })?;

    let network_provisioner = NetlinkNetworkProvisioner;
    let link = network_provisioner
        .get_link_in_ns(&netns, ifname)
        .await
        .map_err(|e| {
            Error::InvalidNetworkConfig(format!(
                "could not open namespace {}: {e}",
                netns_path.display()
            ))
        })?;
    let addresses = link.addresses(None).await.map_err(|e| {
        Error::InvalidNetworkConfig(format!("could not get address of interface {ifname}: {e}"))
    })?;

    let addresses: HashSet<InterfaceAddress> = HashSet::from_iter(addresses);
    let mut want_addresses: Vec<InterfaceAddress> = Vec::new();
    for (index, iface) in prev_result.interfaces.iter().enumerate() {
        if iface.sandbox.is_none() {
            continue;
        }
        if iface.name != ifname {
            continue;
        }
        for ip in &prev_result.ips {
            if ip.interface.map(|i| i as usize) == Some(index) {
                let address = ip.address.parse::<InterfaceAddress>().map_err(|e| {
                    Error::InvalidNetworkConfig(format!("could not parse interface address: {e}"))
                })?;
                want_addresses.push(address);
            }
        }
    }

    let missing: Option<InterfaceAddress> = want_addresses
        .iter()
        .find(|addr| !addresses.contains(addr))
        .copied();
    if let Some(addr) = missing {
        return Err(Error::InvalidNetworkConfig(format!(
            "expected ip {addr} on interface {ifname}"
        )));
    }

    Ok(CNIResult::default())
}
