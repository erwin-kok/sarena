#!/usr/bin/env bash
#
# Installs freshly-built sarena binaries onto every node of the local kind
# cluster, without rebuilding/reloading a Docker image -- a fast inner dev
# loop, same idea as Cilium's own kind dev scripts (which this was
# originally adapted from), just retargeted at what `just build`/`just
# build-ebpf` actually produce here: debug binaries under target/debug and
# a single precompiled eBPF object under target-ebpf/, not a source tree
# compiled on the node.
#
# Expects `cargo build` (sarena-daemon, sarena-cni-plugin) and
# `cargo xtask build-ebpf` to have already run -- `just kind-install`
# depends on `build`/`build-ebpf` so this is normally automatic.
#
# Pass --skip-daemon to install everything except sarena-daemon itself and
# leave the node without one running -- used by `just kind-run-daemon` when
# you want the daemon running locally (attached to the node's network
# namespace via nsenter, see scripts/kind-run-daemon.sh) instead of inside
# it. In that case the conflist also points the CNI plugin at the daemon
# over TCP (127.0.0.1:3000) rather than the default Unix socket: nsenter
# only enters the node's *network* namespace, sharing its loopback (so TCP
# just works), not its mount namespace (so a Unix socket path bound on the
# host isn't visible inside the node at all).

set -euo pipefail

skip_daemon=0
if [[ "${1:-}" == "--skip-daemon" ]]; then
    skip_daemon=1
fi

cluster_name="sarena"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

daemon_bin="${repo_root}/target/debug/sarena-daemon"
cni_bin="${repo_root}/target/debug/sarena-cni"
ebpf_obj="${repo_root}/target-ebpf/sarena-ebpf-programs.o"

have_kind() {
    [[ -n "$(command -v kind)" ]]
}

if ! have_kind; then
    echo "Please install kind first:"
    echo "  https://kind.sigs.k8s.io/docs/user/quick-start/#installation"
    exit 1
fi

have_docker() {
    [[ -n "$(command -v docker)" ]]
}

if ! have_docker; then
    echo "Please install docker first."
    exit 1
fi

required_artifacts=("${cni_bin}" "${ebpf_obj}")
if [[ "${skip_daemon}" -eq 0 ]]; then
    required_artifacts+=("${daemon_bin}")
fi
for f in "${required_artifacts[@]}"; do
    if [[ ! -f "${f}" ]]; then
        echo "missing build artifact: ${f}"
        echo "run 'just build' and 'just build-ebpf' first (or just 'just kind-install', which does both)"
        exit 1
    fi
done

nodes="$(kind get nodes --name "${cluster_name}")"
if [[ -z "${nodes}" ]]; then
    echo "no nodes found for kind cluster '${cluster_name}' -- run 'just kind-up' first"
    exit 1
fi

for node_name in ${nodes}; do
    echo "==> installing sarena onto node ${node_name}"

    docker exec "${node_name}" mkdir -p /usr/lib/sarena/ebpf /opt/cni/bin /etc/cni/net.d

    docker cp "${ebpf_obj}" "${node_name}:/usr/lib/sarena/ebpf/sarena-ebpf-programs.o"

    docker cp "${cni_bin}" "${node_name}:/opt/cni/bin/sarena-cni"
    docker exec "${node_name}" chmod +x /opt/cni/bin/sarena-cni

    # Must match `type`/`name` in sarena-plugins/cni/src/main.rs.
    docker exec -i "${node_name}" sh -c "cat > /etc/cni/net.d/10-sarena.conflist" <<EOF
{
  "cniVersion": "1.0.0",
  "name": "sarena",
  "plugins": [
    {
      "type": "sarena-cni",
      "daemon-endpoint": "tcp://127.0.0.1:3000",
      "enable-debug": true,
      "log-file": "/var/log/sarena-cni.log"
    }
  ]
}
EOF

    # kind/Docker nodes don't come with bpffs mounted at /sys/fs/bpf by
    # default -- same gotcha as deploy/kind/daemonset.yaml works around.
    docker exec "${node_name}" sh -c \
        "mount | grep -q ' /sys/fs/bpf type bpf' || mount -t bpf bpf /sys/fs/bpf"

    if [[ "${skip_daemon}" -eq 1 ]]; then
        # Stop any daemon a previous non---skip-daemon install started, so
        # it doesn't keep holding the socket a locally-run daemon needs.
        docker exec "${node_name}" pkill -x sarena-daemon 2>/dev/null || true
        echo "    skipped sarena-daemon (--skip-daemon) -- run 'just kind-run-daemon' to attach one locally"
        continue
    fi

    docker cp "${daemon_bin}" "${node_name}:/usr/local/bin/sarena-daemon"
    docker exec "${node_name}" chmod +x /usr/local/bin/sarena-daemon

    # Idempotent re-install: stop any previously-started daemon before
    # launching the freshly-copied one.
    docker exec "${node_name}" pkill -x sarena-daemon 2>/dev/null || true

    docker exec -d "${node_name}" env \
        SARENA_SOCKET=/tmp/sarena.sock \
        EBPF_DIR=/usr/lib/sarena/ebpf \
        sh -c "exec /usr/local/bin/sarena-daemon > /var/log/sarena-daemon.log 2>&1"

    echo "    sarena-daemon started on ${node_name} (logs: /var/log/sarena-daemon.log)"
done

echo "==> done. Check node readiness with: kubectl --context kind-${cluster_name} get nodes"
