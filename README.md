# Sarena

[![ci](https://github.com/erwin-kok/sarena/actions/workflows/ci.yaml/badge.svg)](https://github.com/erwin-kok/sarena/actions/workflows/ci.yaml)
[![made-with-rust](https://img.shields.io/badge/Made%20with-Rust-1f425f.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/github/license/erwin-kok/sarena.svg)](https://github.com/erwin-kok/sarena/blob/master/LICENSE-APACHE)


An eBPF-based 🐝 virtual network dataplane and control plane, written in Rust
with [Aya](https://aya-rs.dev/). Sarena is a independent, from-scratch
exploration of how kernel-level networking (routing, ARP, forwarding, and
eventually identity-aware policy) actually works, built one deliberate
stage at a time.

> **Status: early development.** Sarena is an educational project focused on 
> understanding and implementing kernel-level networking concepts in Rust. It
> is not audited, not benchmarked, and not intended for production use.
> Expect incomplete features, rough edges, and breaking changes without
> notice.

## Why this exists

Most understanding of eBPF-based networking comes from reading about
systems like [Cilium](https://cilium.io/) rather than building one. Sarena
exists to close that gap directly: implement the mechanisms — TC
classifiers, BPF maps, ARP handling, longest-prefix-match forwarding — from
first principles, in small, individually verifiable stages, instead of
treating them as a black box behind a CNI plugin.

Sarena is heavily inspired by Cilium's datapath design and deliberately adopts 
several of its architectural ideas. It is not a fork, a replacement, or a 
competitor. Cilium is a mature, production-grade project built by a much larger 
team solving a much larger problem. 

Rather than recreating Cilium feature-for-feature, Sarena focuses on implementing
selected networking mechanisms in a small, understandable codebase. The emphasis
is on understanding, clarity, and sound software engineering rather than feature 
parity.

## What Sarena explores

- How TC/XDP eBPF programs make per-packet forwarding decisions, and how
  that state is pushed down from userspace via BPF maps
- ARP resolution and neighbor table maintenance, done in-kernel
- Longest-prefix-match routing (`LpmTrie`) as the general case of the
  exact-match forwarding that identity-based fabrics like Cilium use for
  already-known endpoints
- Eventually: VXLAN encapsulation, BGP-learned routes, VRF-style route
  partitioning, and an identity/policy layer in the spirit of Cilium's
  endpoint model, layered on top of the router rather than assumed from
  day one

## Setup

In order to build the project, it needs scapy, and Rust nightly.

Scapy needs to be in `scapyenv` (this venv is currently hard-coded).

```shell
python3 -m venv scapyenv
source scapyenv/bin/activate
pip install scapy
```

To install nightly (if not already present):

```shell
rustup toolchain install nightly
```

Check with:
```shell
rustup show
```

Create the following dir and add read/write/execute permissions (The ebpf programs will be stored here):
```shell
sudo mkdir -p /usr/lib/sarena/ebpf
sudo chmod a+rwx /usr/lib/sarena/ebpf
```

## Engineering blog

From time to time, I write about new features, implementation details, design decisions, and lessons learned while exploring and building this project.

If you're interested in the background and technical details, check out my engineering blog:

👉 https://erwinkok.org/

## About the name

*Sarena* comes from two places at once: **arena**, a central space where
separate participants converge and interact — roughly what a router's
forwarding table is, every port meeting at one shared point of
coordination — and **sarang**, Indonesian for *nest*, chosen for personal
reasons tied to a strong connection to Indonesia. Both readings point at
the same idea: a structure that things return to and pass through.

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
  