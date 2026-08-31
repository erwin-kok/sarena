use aya_ebpf::{
    bindings::BPF_F_NO_PREALLOC,
    btf_maps::HashMap,
    macros::{btf_map, map},
    maps::Array,
};
use sarena_shared::{EndpointConfig, EndpointInfo, Ipv4Key};

use crate::error::{EbpfError, Res};

const LXC_MAP_SIZE: usize = 65536;

#[btf_map(name = "lxc_map")]
static LXC_MAP: HashMap<Ipv4Key, EndpointInfo, LXC_MAP_SIZE, { BPF_F_NO_PREALLOC as usize }> =
    HashMap::new();

#[map(name = "endpoint_config")]
static ENDPOINT_CONFIG: Array<EndpointConfig> = Array::with_max_entries(1, 0);

#[inline(always)]
pub fn lookup_ipv4_endpoint(ip: Ipv4Key) -> Option<*const EndpointInfo> {
    LXC_MAP.get_ptr(&ip)
}

#[inline(always)]
pub fn get_endpoint_config<'a>() -> Res<&'a EndpointConfig> {
    ENDPOINT_CONFIG.get(0).ok_or(EbpfError::InternalError(
        "endpoint does not have EndpointConfig",
    ))
}
