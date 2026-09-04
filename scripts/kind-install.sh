#!/usr/bin/env bash

set -euo pipefail

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

    docker exec "${node_name}" sh -c \
        "mount | grep -q ' /sys/fs/bpf type bpf' || mount -t bpf bpf /sys/fs/bpf"
done

echo "==> done. Check node readiness with: kubectl --context kind-${cluster_name} get nodes"
