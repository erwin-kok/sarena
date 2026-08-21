use sarena_infra::NetlinkNetworkProvisioner;
use sarena_loader::LoaderHandle;

#[derive(Clone)]
pub struct AppState {
    pub loader_handle: LoaderHandle,
    pub netlink_provisioner: NetlinkNetworkProvisioner,
}

impl AppState {
    pub fn new(
        loader_handle: LoaderHandle,
        netlink_provisioner: NetlinkNetworkProvisioner,
    ) -> Self {
        Self {
            loader_handle,
            netlink_provisioner,
        }
    }
}
