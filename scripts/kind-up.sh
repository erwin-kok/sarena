#!/usr/bin/env bash

set -euo pipefail

cluster_name="sarena"
pod_subnet="10.244.0.0/16"
service_subnet=""

have_kind() {
    [[ -n "$(command -v kind)" ]]
}

if ! have_kind; then
    echo "Please install kind first:"
    echo "  https://kind.sigs.k8s.io/docs/user/quick-start/#installation"
    exit 1
fi

have_kubectl() {
    [[ -n "$(command -v kubectl)" ]]
}

if ! have_kubectl; then
    echo "Please install kubectl first:"
    echo "  https://kubernetes.io/docs/tasks/tools/#kubectl"
    exit 1
fi

kind_cmd="kind create cluster"
kind_cmd+=" --name ${cluster_name}"

cat <<EOF | ${kind_cmd} --config=-
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
nodes:
  - role: control-plane
networking:
  disableDefaultCNI: true
  kubeProxyMode: iptables
  ipFamily: ipv4
  ${pod_subnet:+"podSubnet: "$pod_subnet}
  ${service_subnet:+"serviceSubnet: "$service_subnet}
  apiServerAddress: 127.0.0.1
  apiServerPort: 0

kubeadmConfigPatches:
  - |
    kind: ClusterConfiguration
    metadata:
      name: config
    apiServer:
      extraArgs:
        "v": "3"
    controllerManager:
      extraArgs:
        authorization-always-allow-paths: /healthz,/readyz,/livez,/metrics
        bind-address: 0.0.0.0
    scheduler:
      extraArgs:
        authorization-always-allow-paths: /healthz,/readyz,/livez,/metrics
        bind-address: 0.0.0.0
  - |
    kind: InitConfiguration
    nodeRegistration:
      kubeletExtraArgs:
        container-log-max-size: "10M"
EOF

set +e
kubectl taint nodes --all node-role.kubernetes.io/control-plane- 2>/dev/null
kubectl taint nodes --all node-role.kubernetes.io/master- 2>/dev/null
set -e
