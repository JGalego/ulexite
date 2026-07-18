#!/usr/bin/env bash
# Regenerates the demo GIFs under examples/demos/ from their .tape scripts
# (https://github.com/charmbracelet/vhs) and syncs the result into
# website/static/img/demos/, which is a checked-in copy the docs site reads
# directly (website/build/ is gitignored and rebuilt by `just docs-build`,
# so it's left alone here).
#
# Most demos call a real vendor (`--no-cache`, no `--mock`) so the GIF shows
# a genuine response instead of the deterministic mock — this costs real
# API calls and needs the matching key(s) exported first. See
# examples/README.md's "Running one" section for exactly which vendor each
# one uses.
#
# Usage:
#   scripts/regen-demos.sh                 # regenerate every demo
#   scripts/regen-demos.sh translate batch # regenerate just these
#   scripts/regen-demos.sh --no-install ... # skip the `cargo install` step
#   scripts/regen-demos.sh --list          # list demo names and required keys
set -euo pipefail

say() { printf '%s\n' "$*" >&2; }
die() {
  say "error: $*"
  exit 1
}
need() {
  command -v "$1" >/dev/null 2>&1 || die "this script requires '$1', please install it first"
}

need git

repo_root="$(git rev-parse --show-toplevel)"
demos_dir="$repo_root/examples/demos"
site_copy_dir="$repo_root/website/static/img/demos"

# name -> space-separated env vars ulx needs to hit a real vendor for that
# demo (kept in sync with examples/*.ulx's `api_key_env` decls and the
# `--provider`/default choices in each .tape — see examples/README.md).
declare -A REQUIRED_KEYS=(
  [approval]="ANTHROPIC_API_KEY"
  [batch]="ANTHROPIC_API_KEY"
  [custom_provider]=""
  [eval_translate]="ANTHROPIC_API_KEY"
  [generate_and_describe]="OPENAI_API_KEY ANTHROPIC_API_KEY"
  [multi_agent]="ANTHROPIC_API_KEY"
  [pdf_qa]="ANTHROPIC_API_KEY GROQ_API_KEY"
  [prompt_from_file]="ANTHROPIC_API_KEY"
  [rag]="ANTHROPIC_API_KEY OPENAI_API_KEY GROQ_API_KEY"
  [summarize]="ANTHROPIC_API_KEY GROQ_API_KEY"
  [translate]="ANTHROPIC_API_KEY"
  [voice_memo]="GROQ_API_KEY OPENAI_API_KEY"
)

install_ulx=1
names=()
list_only=0
for arg in "$@"; do
  case "$arg" in
  --no-install) install_ulx=0 ;;
  --list) list_only=1 ;;
  -h | --help)
    say "Usage: $0 [--no-install] [--list] [demo-name ...]"
    exit 0
    ;;
  *) names+=("$arg") ;;
  esac
done

if [ "$list_only" -eq 1 ]; then
  for f in "$demos_dir"/*.tape; do
    n="$(basename "$f" .tape)"
    printf '%-24s %s\n' "$n" "${REQUIRED_KEYS[$n]-<unknown>}"
  done
  exit 0
fi

if [ "${#names[@]}" -eq 0 ]; then
  for f in "$demos_dir"/*.tape; do
    names+=("$(basename "$f" .tape)")
  done
fi

missing_overall=""
for n in "${names[@]}"; do
  [ -f "$demos_dir/$n.tape" ] || die "no such demo '$n' (no $demos_dir/$n.tape)"
  for key in ${REQUIRED_KEYS[$n]:-}; do
    if [ -z "${!key:-}" ]; then
      missing_overall="$missing_overall $n:$key"
    fi
  done
done
if [ -n "$missing_overall" ]; then
  say "missing required env var(s):$missing_overall"
  die "export the key(s) above (see examples/README.md's 'Running one'), or pass --list to see the full map"
fi

need vhs

if [ "$install_ulx" -eq 1 ]; then
  say "building and installing ulx from crates/ulx-cli..."
  (cd "$repo_root" && cargo install --path crates/ulx-cli --locked --quiet)
fi
need ulx

cd "$repo_root"
for n in "${names[@]}"; do
  say "recording $n.gif..."
  vhs "$demos_dir/$n.tape"
done

say "syncing regenerated GIFs to website/static/img/demos/..."
for n in "${names[@]}"; do
  cp "$demos_dir/$n.gif" "$site_copy_dir/$n.gif"
done

say "done. review with: git status examples/demos website/static/img/demos"
