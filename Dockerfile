# syntax=docker/dockerfile:1
#
# Assembles the sarena runtime image from artifacts already built on the
# host — `scripts/image-build.sh` cross-compiles sarena-daemon/sarena-cni as
# static musl binaries (`cargo build --target x86_64-unknown-linux-musl`),
# with the eBPF object (`cargo xtask build-ebpf`) embedded in sarena-daemon,
# then stages them into ./dist before invoking this build. Because the binaries are static,
# the runtime base's libc doesn't matter and no compiler toolchain is
# needed here.
#
# Build context for this Dockerfile is ./dist, not the repo root:
#   docker build -f Dockerfile ./dist

FROM docker.io/alpine:3.22

RUN apk add --no-cache ca-certificates

COPY sarena-daemon /usr/local/bin/sarena-daemon
COPY sarena-cni /usr/local/bin/sarena-cni
COPY install-cni.sh /usr/local/bin/install-cni.sh
RUN chmod 0755 /usr/local/bin/sarena-daemon /usr/local/bin/sarena-cni /usr/local/bin/install-cni.sh

ENTRYPOINT ["/usr/local/bin/sarena-daemon"]
