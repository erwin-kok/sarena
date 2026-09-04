#!/usr/bin/env bash

set -euo pipefail

cluster_name="sarena"
node_name="${cluster_name}-control-plane"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cli_bin="${repo_root}/target/debug/sarena-cli"
tcp_port=3000

have_nsenter() {
    [[ -n "$(command -v nsenter)" ]]
}

if ! have_nsenter; then
    echo "nsenter not found (usually part of util-linux)."
    exit 1
fi

if [[ ! -f "${cli_bin}" ]]; then
    echo "missing build artifact: ${cli_bin}"
    echo "run 'just build' first"
    exit 1
fi

node_pid="$(docker inspect -f '{{.State.Pid}}' "${node_name}" 2>/dev/null)" || {
    echo "kind node '${node_name}' not found/running -- run 'just kind-up' first"
    exit 1
}

exec sudo nsenter --target "${node_pid}" --net -- \
    env SARENA_HOST="tcp://127.0.0.1:${tcp_port}" "${cli_bin}" "$@"
