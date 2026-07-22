export PATH := justfile_directory() / "scapyenv/bin" + ":" + env_var("PATH")

# Build all workspace packages (excluding eBPF programs)
build:
    cargo build

# Quick compilation check without producing binaries
check:
    cargo check

# clean up target
clean:
    cargo clean

# fmt up target
fmt:
    cargo fmt

# clippy
clippy:
    cargo clippy

# Run all tests except the eBPF test runner (requires root + installed eBPF programs)
test:
    cargo test --workspace \
        --exclude sarena-test-runner \
        --exclude sarena-ebpf-programs \
        --exclude sarena-ebpf-test-programs

# Build eBPF programs (outputs to ./target-ebpf/)
build-ebpf:
    cargo xtask build-ebpf

# Build and install eBPF programs to /usr/lib/sarena/ebpf (requires sudo)
install-ebpf: build-ebpf
    cargo xtask install-ebpf

ebpf-test:
    #!/usr/bin/env bash
    set -euo pipefail
    exe=$(cargo test --no-run -p sarena-test-runner --message-format=json \
        | jq -r 'select(.profile.test == true) | .executable')
    sudo "$exe" --no-capture

# Full workflow: build, test, install eBPF programs, run eBPF tests
all: build test install-ebpf ebpf-test
