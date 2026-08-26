#!/usr/bin/env bash
#
# Runs the locally-built sarena-cli (target/debug/sarena-cli) attached to
# the kind node's *network* namespace via nsenter, so it can reach the
# sarena-daemon started by `just kind-run-daemon`. That daemon listens on
# tcp://127.0.0.1:3000 inside the node's network namespace (see
# kind-run-daemon.sh's comments for why TCP-on-loopback rather than a
# Unix socket), which isn't reachable from this host's own network
# namespace without also being attached to the node's.
#
# We deliberately do NOT also enter the node's mount namespace, same
# reasoning as kind-run-daemon.sh: the cli binary loads straight from
# this host's target/debug/, no need for that path to exist inside the
# node.
#
# All arguments are forwarded to sarena-cli, e.g.:
#   just kind-sc service list

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
