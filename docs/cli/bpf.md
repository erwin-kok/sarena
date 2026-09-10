# `bpf` — direct access to local BPF maps

Operates on the datapath's pinned maps under `pin_root` (default
`/sys/fs/bpf/sarena`, see [config](../config/README.md)). These subcommands read
and write kernel state directly and therefore **must be run as root**; they do
not talk to the daemon.

#### `bpf endpoint` (alias `bpf ep`) — local endpoint map

| Command | Description |
| --- | --- |
| `bpf endpoint list` (alias `ls`) `[-o <FORMAT>]` | List every entry in the local endpoint (`lxc_map`) map: IPv4 address → local endpoint info. Fails if the datapath is not loaded. |
| `bpf endpoint delete <IP>` | Remove the entry for `<IP>` (an IPv4 address) from the local endpoint map. |

```console
$ sudo sarena-cli bpf ep ls
IP ADDRESS   LOCAL ENDPOINT INFO
10.0.0.2     ...

$ sudo sarena-cli bpf endpoint delete 10.0.0.2
```

#### `bpf metrics` — datapath traffic metrics

Reads the per-CPU `metrics_map`, which the datapath increments at each
observation point as packets pass through it.

| Command | Description |
| --- | --- |
| `bpf metrics list` (alias `ls`) `[-o <FORMAT>]` | List datapath traffic metrics: one row per observation point, with aggregated packet and byte counts. Fails if the datapath is not loaded. |
| `bpf metrics flush` | Clear all datapath traffic metrics (zero every entry in `metrics_map`). |

Observation points: 
(this list could be extended in the future. Should be aligned with `sarena-shared/src/metrics.rs`):

| `OBS-POINT` | Meaning |
| --- | --- |
| `CONTAINER` | Container-facing forwarding path. |
| `HOST` | Host-facing forwarding path. |
| `UNKNOWN` | Unrecognised observation-point id (should not normally appear). |

```console
$ sudo sarena-cli bpf metrics ls
OBS-POINT   PACKETS   BYTES
CONTAINER   1234      567890
HOST        4321      98765

$ sudo sarena-cli bpf metrics flush
```
