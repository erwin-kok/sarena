use aya_ebpf::{bindings::BPF_F_NO_PREALLOC, btf_maps::HashMap, macros::btf_map};
use sarena_shared::{EndpointInfo, Ipv4Key};

const ENDPOINTS_MAP_SIZE: usize = 65536;

#[btf_map(name = "lxc_map")]
static LXC_MAP: HashMap<Ipv4Key, EndpointInfo, ENDPOINTS_MAP_SIZE, { BPF_F_NO_PREALLOC as usize }> =
    HashMap::new();

pub fn lookup_ipv4_endpoint(ip: Ipv4Key) -> Option<*const EndpointInfo> {
    LXC_MAP.get_ptr(ip)
}
