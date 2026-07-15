# Common dev tasks — run `just` to list them, `just <recipe>` to run one.
# Mirrors what .github/workflows/ci.yml checks, so `just ci` is a local
# dry-run of the whole pipeline before you push.

default:
    @just --list

# Build every crate in the workspace.
build:
    cargo build --workspace

# Run every test in the workspace.
test:
    cargo test --workspace

# Format the whole workspace in place.
fmt:
    cargo fmt --all

# Fail if anything isn't formatted (what CI runs).
fmt-check:
    cargo fmt --all -- --check

# Clippy, denying warnings (what CI runs).
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# fmt-check + clippy + build + test, in that order — the same gate CI enforces.
ci: fmt-check clippy build test

# Install a real `ulx` binary onto your PATH (rerun after pulling changes).
install:
    cargo install --path crates/ulx-cli --locked

# `ulx check` every example under examples/ — mirrored as raw commands in
# .github/workflows/ci.yml's `spec-examples` job (not invoked directly,
# same reasoning as `ci` above); keep the two in sync by hand.
check-examples:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build -p ulx-cli
    for f in examples/*.ulx; do
        echo "checking $f"
        ./target/debug/ulx check "$f"
    done

# Remove build artifacts and local runtime state (.ulexite/).
clean:
    cargo clean
    rm -rf .ulexite
