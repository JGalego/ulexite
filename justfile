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

# `ulx check` every example under examples/ (both hand-written .ulx and
# .md compiled via `ulx from-md`) — mirrored as raw commands in
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
    for f in examples/*.md; do
        [ "$(basename "$f")" = "README.md" ] && continue
        echo "checking $f"
        out="$(mktemp --suffix .ulx)"
        ./target/debug/ulx from-md "$f" -o "$out"
        ./target/debug/ulx check "$out"
        rm -f "$out"
    done

# Build the WASM playground crate + the Docusaurus site — mirrored as raw
# commands in .github/workflows/pages-deploy.yml's `build` job (not
# invoked directly, same reasoning as `ci` above); keep the two in sync
# by hand. Needs Node >=20 on PATH (Docusaurus 3.10's requirement) and
# `wasm-pack` installed (`cargo install wasm-pack`).
docs-build:
    wasm-pack build crates/ulx-wasm --target web --out-dir ../../website/static/wasm
    cd website && npm ci && npm run build

# Same build as `docs-build`, then serve it locally with hot reload —
# for iterating on site content/the playground without a full rebuild
# per change.
docs-serve:
    wasm-pack build crates/ulx-wasm --target web --out-dir ../../website/static/wasm
    cd website && npm install && npm start

# Remove build artifacts and local runtime state (.ulexite/).
clean:
    cargo clean
    rm -rf .ulexite
