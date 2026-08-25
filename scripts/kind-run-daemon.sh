#!/usr/bin/env bash
#
# Runs a locally-built sarena-daemon (target/debug/sarena-daemon, no
# `docker cp` involved) attached to the kind node's *network* namespace via
# nsenter, while keeping this host's own mount namespace. That combination
# matters:
#
#   - The CNI plugin (invoked by kubelet *inside* the kind node) creates
#     each pod's host-side veth end in the node's network namespace, not
#     this host's. sarena-daemon's netlink/eBPF-attach calls
#     (NetlinkNetworkProvisioner::get_link, TCX attach) only see interfaces
#     in whatever network namespace the *daemon process itself* is
#     currently in -- hence `nsenter --net`, so it looks in the node's.
#   - We deliberately do NOT also enter the node's mount namespace: BPF
#     link attachment is keyed by (network namespace, ifindex) in the
#     kernel, not by which bpffs a pin lives under, so pinning under this
#     host's own /sys/fs/bpf/sarena works fine and is simpler -- it also
#     means the daemon binary loads straight from this host's target/debug/
#     (no need for that path to exist inside the node).
#   - The CNI plugin still needs to reach this daemon. A Unix socket bound
#     here would only exist on this host's own filesystem, invisible inside
#     the node (we didn't enter its mount namespace) -- but `--net` *does*
#     put us on the node's network namespace, which has its own loopback
#     shared with everything else running in it (including the CNI plugin).
#     So TCP on 127.0.0.1 just works with no bridging at all. This is why
#     `scripts/kind-install.sh --skip-daemon` (run below) writes the
#     conflist with "daemon-endpoint": "tcp://127.0.0.1:3000" instead of
#     leaving the CNI plugin on its Unix-socket default.
#
# Requires this host to have bpffs mounted at /sys/fs/bpf (`mount | grep
# bpf` -- true on most modern systemd distros already).

set -euo pipefail

cluster_name="sarena"
node_name="${cluster_name}-control-plane"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
daemon_bin="${repo_root}/target/debug/sarena-daemon"
tcp_port=3000

have_nsenter() {
    [[ -n "$(command -v nsenter)" ]]
}

if ! have_nsenter; then
    echo "nsenter not found (usually part of util-linux)."
    exit 1
fi

if [[ ! -f "${daemon_bin}" ]]; then
    echo "missing build artifact: ${daemon_bin}"
    echo "run 'just build' first"
    exit 1
fi

node_pid="$(docker inspect -f '{{.State.Pid}}' "${node_name}" 2>/dev/null)" || {
    echo "kind node '${node_name}' not found/running -- run 'just kind-up' first"
    exit 1
}

echo "==> running sarena-daemon locally (${daemon_bin})"
echo "    attached to ${node_name}'s network namespace (pid ${node_pid})"
echo "    eBPF object from: ${repo_root}/target-ebpf (run 'just build-ebpf' first)"
echo "    reachable from the node (and the CNI plugin) at tcp://127.0.0.1:${tcp_port}"
echo

exec sudo nsenter --target "${node_pid}" --net -- \
    env EBPF_DIR="${repo_root}/target-ebpf" "${daemon_bin}"
