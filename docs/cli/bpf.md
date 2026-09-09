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
