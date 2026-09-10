use aya_ebpf::{bindings::BPF_F_NO_PREALLOC, btf_maps::PerCpuHashMap, macros::btf_map};
use sarena_shared::{MetricsKey, MetricsValue};

const METRICS_MAP_SIZE: usize = 65536;

#[btf_map(name = "metrics_map")]
static METRICS_MAP: PerCpuHashMap<
    MetricsKey,
    MetricsValue,
    METRICS_MAP_SIZE,
    { BPF_F_NO_PREALLOC as usize },
> = PerCpuHashMap::new();

#[inline]
pub fn update_metrics(bytes: u64, obs_point: u8) {
    let key = MetricsKey {
        obs_point,
        pad: [0u8; 7],
    };
    if let Some(entry) = METRICS_MAP.get_ptr_mut(&key) {
        unsafe {
            (*entry).packets += 1;
            (*entry).bytes += bytes;
        }
    } else {
        let entry = MetricsValue { packets: 1, bytes };
        let _ = METRICS_MAP.insert(&key, &entry, 0);
    }
}
