#!/usr/bin/env bash

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
