# Sarena

An eBPF-based 🐝 virtual network dataplane and control plane, written in Rust
with [Aya](https://aya-rs.dev/). Sarena is a personal, from-scratch
exploration of how kernel-level networking — routing, ARP, forwarding, and
eventually identity-aware policy — actually works, built one deliberate
stage at a time.

> **Status: early development.** Sarena is a hobby and learning project. It
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

Sarena is heavily inspired by Cilium's datapath design and deliberately
borrows several of its ideas. It is not a fork, a replacement, or a competitor.
Cilium is a mature, production-grade project built by a much larger team
solving a much larger problem. Sarena's goal is understanding, not
adoption.

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

## About the name

*Sarena* comes from two places at once: **arena**, a central space where
separate participants converge and interact — roughly what a router's
forwarding table is, every port meeting at one shared point of
coordination — and **sarang**, Indonesian for *nest*, chosen for personal
reasons tied to a strong connection to Indonesia. Both readings point at
the same idea: a structure that things return to and pass through.

## License

Not finalized yet. Likely a dual MIT/Apache-2.0 license for the
control-plane crates and MIT/GPL for the eBPF crate, matching common
practice in the Aya ecosystem — final decision pending.

## Acknowledgments

- [Cilium](https://github.com/cilium/cilium) — the primary reference and
  inspiration for this project's design
- [Aya](https://github.com/aya-rs/aya) — the Rust eBPF library this project
  is built on
  