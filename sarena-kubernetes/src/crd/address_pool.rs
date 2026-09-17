use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "sarena.erwinkok.org",
    version = "v1alpha1",
    kind = "AddressPool"
)]
#[kube(status = "AddressPoolStatus")]
#[kube(
    printcolumn = r#"{"name":"CIDR", "type":"string", "description":"CIDR of Address pool", "jsonPath":".spec.cidr"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct AddressPoolSpec {
    pub cidr: String,
}

#[derive(Deserialize, Serialize, Clone, Default, Debug, JsonSchema)]
pub struct AddressPoolStatus {}
