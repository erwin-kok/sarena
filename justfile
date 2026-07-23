export PATH := justfile_directory() / "scapyenv/bin" + ":" + env_var("PATH")

setup:
    python -m venv scapyenv
    scapyenv/bin/pip install -r scapy/requirements.txt

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
        --features test \
        --exclude sarena-test-runner \
        --exclude sarena-ebpf-programs \
        --exclude sarena-ebpf-test-programs

# Build eBPF programs (outputs to ./target-ebpf/)
build-ebpf:
    cargo xtask build-ebpf

# Build and install eBPF programs to /usr/lib/sarena/ebpf (requires sudo)
install-ebpf: build-ebpf
    cargo xtask install-ebpf

netns-clean:
    #!/usr/bin/env bash
    set -euo pipefail

    for _ in $(seq 1 20); do
        mounts=$(awk '{print $5}' /proc/self/mountinfo | grep -E '^/run/netns(/|$)' || true)
        if [ -z "$mounts" ]; then
            exit 0
        fi
        while IFS= read -r m; do
            sudo umount "$m" 2>/dev/null || true
        done < <(echo "$mounts" | awk '{ print length, $0 }' | sort -rn | cut -d' ' -f2-)
    done

    echo "warning: could not fully clean up mounts under /run/netns:" >&2
    awk '{print $5}' /proc/self/mountinfo | grep -E '^/run/netns(/|$)' >&2 || true
    exit 1
    
# Run all integration tests in the sarena-infra package (requires root)
infra-test: netns-clean
    #!/usr/bin/env bash
    set -euo pipefail
    exes=$(cargo test -p sarena-infra --features test --tests --no-run --message-format=json \
        | jq -r 'select(.profile.test == true) | .executable | select(. != null)')
    for exe in $exes; do
        just netns-clean
        sudo "$exe" --ignored --no-capture
    done

ebpf-test:
    #!/usr/bin/env bash
    set -euo pipefail
    exe=$(cargo test --no-run -p sarena-test-runner --message-format=json \
        | jq -r 'select(.profile.test == true) | .executable')
    sudo "$exe" --no-capture

# Full workflow: build, test, install eBPF programs, run eBPF tests
all: build test install-ebpf infra-test ebpf-test
