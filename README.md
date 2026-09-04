# Sarena

[![ci](https://github.com/erwin-kok/sarena/actions/workflows/ci.yaml/badge.svg)](https://github.com/erwin-kok/sarena/actions/workflows/ci.yaml)
[![made-with-rust](https://img.shields.io/badge/Made%20with-Rust-1f425f.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/github/license/erwin-kok/sarena.svg)](https://github.com/erwin-kok/sarena/blob/master/LICENSE-APACHE)

An eBPF-based 🐝 virtual network dataplane, control plane and CNI, written in Rust 🦀
using [Aya](https://aya-rs.dev/). 

Sarena is an independent, from-scratch exploration of how kernel-level networking (routing, ARP, forwarding, and others) actually works, built one feature at a time.

> **Status: early development.** Sarena is an educational project focused on 
> understanding and implementing kernel-level networking concepts in Rust especially
> around eBPF. It is not audited, not benchmarked, and not intended for production use.
> Expect incomplete features, rough edges, and breaking changes without
> notice.

## Why this exists

A lot of the way to learn eBPF networking is by reading about projects like Cilium and looking at how they work. But I wanted to actually build one myself.

Sarena is my attempt to understand what is happening underneath: TCX classifiers, BPF maps, ARP, and the other pieces that make a network dataplane work. The idea is to build these things one at a time, rather than hiding everything behind a CNI plugin.

Sarena is heavily inspired by Cilium's datapath and uses some of the same architectural ideas. It is not a fork, replacement, or competitor. Cilium is a mature production project built by a much larger team and solves a much bigger problem.

I'm not trying to recreate Cilium feature by feature. I mainly want a small codebase where I can experiment with and understand the individual pieces of an eBPF-based network dataplane.

## Prerequisites

In order to build the project, it needs the following:

* **Rust toolchains**, via [rustup](https://rustup.rs/):
```shell
rustup toolchain install stable
rustup toolchain install nightly --component rust-src
```

* **bpf-linker** — the eBPF programs' linker:
```shell
cargo install bpf-linker
```

* **just** - to build the project:
```shell
cargo install just
```

* **Docker**, **[kind](https://kind.sigs.k8s.io/docs/user/quick-start/#installation)**,
and **kubectl**.

## Setup

Create the following dir and add read/write/execute permissions (The ebpf programs will be stored here):

```shell
sudo mkdir -p /usr/lib/sarena/ebpf
sudo chmod a+rwx /usr/lib/sarena/ebpf
```

Scapy needs to be in `scapyenv` (this venv is currently hard-coded). To create this:

```shell
just setup
```

## Building & Testing

To build and test everything, just do:

> Note that you will need **root privileges** in order to install and test eBPF programs.

```shell
just all
```

This will build all the crates, inluding the eBPF crates. It installs the eBPF program in
 `/usr/lib/sarena/ebpf` and it runs all the tests (normal unit tests, and also eBPF tests).

## Testing and running in Kubernetes

> The following runs the Sarena daemon locally on your system, and enters the network namespace of the control plane otherwise the CNI connect connect with the daemon. This is great for development purposes. Note that you will need **root privileges** and also know that the **eBPF programs and maps** are used/pinned **locally**.

Note that running the daemon locally you will need **root privileges** in order to install and test eBPF programs

In order ro test in a kind cluster, do the following in sequence:

```shell
just kind-up            # This will spin up a one node kind cluster and does not install a CNI.
just kind-run-daemon    # This will run the Sarena daemon
```

...and keep this running.

Then, in another terminal do:

```shell
kubectl get nodes       # It should show "Unready"
just kind-install       # This will install the Sarena CNI plugin into the Kind node
kubectl get nodes       # Now it should show "Ready"
```

If you do:

```shell 
kubectl get all --all-namespaces
```

It should show:

```text
NAMESPACE            NAME                                               READY   STATUS    RESTARTS   AGE
kube-system          pod/coredns-7d764666f9-47js2                       1/1     Running   0          49s
kube-system          pod/coredns-7d764666f9-9dqgq                       1/1     Running   0          49s
kube-system          pod/etcd-sarena-control-plane                      1/1     Running   0          56s
kube-system          pod/kube-apiserver-sarena-control-plane            1/1     Running   0          56s
kube-system          pod/kube-controller-manager-sarena-control-plane   1/1     Running   0          55s
kube-system          pod/kube-proxy-q44jg                               1/1     Running   0          49s
kube-system          pod/kube-scheduler-sarena-control-plane            1/1     Running   0          56s
local-path-storage   pod/local-path-provisioner-67b8995b4b-dpk5w        1/1     Running   0          49s

NAMESPACE     NAME                 TYPE        CLUSTER-IP   EXTERNAL-IP   PORT(S)                  AGE
default       service/kubernetes   ClusterIP   10.96.0.1    <none>        443/TCP                  56s
kube-system   service/kube-dns     ClusterIP   10.96.0.10   <none>        53/UDP,53/TCP,9153/TCP   55s

NAMESPACE     NAME                        DESIRED   CURRENT   READY   UP-TO-DATE   AVAILABLE   NODE SELECTOR            AGE
kube-system   daemonset.apps/kube-proxy   1         1         1       1            1           kubernetes.io/os=linux   55s

NAMESPACE            NAME                                     READY   UP-TO-DATE   AVAILABLE   AGE
kube-system          deployment.apps/coredns                  2/2     2            2           55s
local-path-storage   deployment.apps/local-path-provisioner   1/1     1            1           53s

NAMESPACE            NAME                                                DESIRED   CURRENT   READY   AGE
kube-system          replicaset.apps/coredns-7d764666f9                  2         2         2       49s
local-path-storage   replicaset.apps/local-path-provisioner-67b8995b4b   1         1         1       49s

```

To bring down the kind cluster:

```shell
just kind-down
```

## Engineering blog

From time to time, I write about new features, implementation details, design decisions, and lessons learned while exploring and building this project.

If you're interested in the background and technical details, check out my engineering blog:

👉 https://erwinkok.org/

## About the name

*Sarena* comes from a combination of two words: **arena**, a central space where people meet and interact — roughly what a router's is. And **sarang**, Indonesian for *nest*, chosen for personal reasons because of a strong connection to Indonesia. Both point at the same idea: a structure that things return to and pass through.

## License

Unless otherwise noted, Sarena is dual licensed under either the MIT License or 
the Apache License, Version 2.0, at your option.

Some files derived from third-party projects remain under their original license 
terms, as indicated by their file headers.

Unless you explicitly state otherwise, any contribution intentionally 
submitted for inclusion in this project shall be dual licensed under the 
MIT License and Apache License, Version 2.0, without any additional terms 
or conditions.

## Acknowledgments

Sarena is an independent educational project and is not affiliated with or 
endorsed by the Cilium or Aya projects. Small portions of the repository are 
derived from upstream projects and retain their original copyright notices and 
license headers.

Special thanks to the Cilium community for building and openly sharing a 
production-grade eBPF networking platform that serves as an invaluable learning 
resource.

- [Cilium](https://github.com/cilium/cilium) — the primary reference and
  inspiration for this project's design
- [Aya](https://github.com/aya-rs/aya) — the Rust eBPF library this project
  is built on
  