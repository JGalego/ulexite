#!/usr/bin/env bash
# Installs prebuilt `ulx` and `ulx-lsp` binaries (Linux/macOS) from GitHub
# Releases. `ulx-lsp` is what the Ulexite VS Code extension (and any other
# LSP-capable editor) execs by name off PATH -- installing both here means
# one command sets up both the CLI and everything the editor extension
# needs.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/JGalego/ulexite/main/scripts/install.sh | sh
#
# Env vars:
#   ULX_VERSION     release tag to install, e.g. "v0.1.0" (default: latest)
#   ULX_INSTALL_DIR directory to install the binary into (default: see below)

# POSIX `sh` (e.g. dash on Debian/Ubuntu/WSL) doesn't support `pipefail`, and
# the documented `curl | sh` invocation feeds this script to `sh` directly,
# bypassing the `#!/usr/bin/env bash` shebang above entirely. Nothing in this
# script relies on a multi-command pipeline's exit code, so plain `-eu` is
# both sufficient and portable to every `sh` this is meant to run under.
set -eu

REPO="JGalego/ulexite"
VERSION="${ULX_VERSION:-latest}"

say() { printf '%s\n' "$*" >&2; }
die() {
  say "error: $*"
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "this script requires '$1', please install it first"
}

need curl
need tar
need install

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os-$arch" in
  Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
  Linux-aarch64 | Linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
  Darwin-x86_64) echo "x86_64-apple-darwin" ;;
  Darwin-arm64) echo "aarch64-apple-darwin" ;;
  *) die "unsupported platform: $os-$arch (see docs/spec/24-limitations.md, or build from source with 'cargo install --git https://github.com/${REPO} ulx-cli')" ;;
  esac
}

default_install_dir() {
  if [ -n "${ULX_INSTALL_DIR:-}" ]; then
    printf '%s' "$ULX_INSTALL_DIR"
  elif [ -w "/usr/local/bin" ] 2>/dev/null; then
    printf '%s' "/usr/local/bin"
  else
    printf '%s' "$HOME/.local/bin"
  fi
}

target="$(detect_target)"
install_dir="$(default_install_dir)"

if [ "$VERSION" = "latest" ]; then
  asset_url="https://github.com/${REPO}/releases/latest/download/ulx-${target}.tar.gz"
else
  asset_url="https://github.com/${REPO}/releases/download/${VERSION}/ulx-${target}.tar.gz"
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

say "downloading ${asset_url}"
curl -fsSL "$asset_url" -o "$workdir/ulx.tar.gz" ||
  die "download failed — is there a published release yet? see https://github.com/${REPO}/releases"

tar xzf "$workdir/ulx.tar.gz" -C "$workdir"

mkdir -p "$install_dir"
install -m 755 "$workdir/ulx-${target}/ulx" "$install_dir/ulx"
install -m 755 "$workdir/ulx-${target}/ulx-lsp" "$install_dir/ulx-lsp"

say "installed ulx and ulx-lsp to ${install_dir}"

case ":$PATH:" in
*":$install_dir:"*) ;;
*) say "note: ${install_dir} is not on your PATH — add it, e.g.: export PATH=\"${install_dir}:\$PATH\"" ;;
esac

"$install_dir/ulx" --help >/dev/null 2>&1 || true
say "done. try: ulx --help"
say "for editor support, install the Ulexite VS Code extension too — see the README"
