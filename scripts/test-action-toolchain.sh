#!/usr/bin/env bash
# Exercise the composite action's Rust toolchain resolver against real and adversarial pins.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
resolver="$repo_root/scripts/resolve-action-toolchain.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

assert_resolves() {
  local requested="$1"
  local workspace="$2"
  local expected="$3"
  local output="$tmp/github-output"
  : > "$output"
  GITHUB_OUTPUT="$output" \
    REQUESTED_TOOLCHAIN="$requested" \
    GITHUB_WORKSPACE="$workspace" \
    "$resolver" >/dev/null
  if ! grep -Fx "toolchain=$expected" "$output" >/dev/null; then
    echo "expected toolchain=$expected, got: $(cat "$output")" >&2
    exit 1
  fi
}

assert_fails() {
  local requested="$1"
  local workspace="$2"
  local expected_message="$3"
  local output="$tmp/github-output"
  local stderr="$tmp/stderr"
  : > "$output"
  if GITHUB_OUTPUT="$output" \
    REQUESTED_TOOLCHAIN="$requested" \
    GITHUB_WORKSPACE="$workspace" \
    "$resolver" >/dev/null 2>"$stderr"; then
    echo "expected toolchain resolution to fail" >&2
    exit 1
  fi
  grep -F "$expected_message" "$stderr" >/dev/null
}

workspace() {
  local name="$1"
  local dir="$tmp/$name"
  rm -rf "$dir"
  mkdir -p "$dir"
  echo "$dir"
}

# No repository pin: the historical default.
unpinned="$(workspace unpinned)"
assert_resolves auto "$unpinned" stable

# A repository-root rust-toolchain.toml is honored rather than overridden.
pinned="$(workspace pinned)"
printf '[toolchain]\nchannel = "1.95.0"\ncomponents = ["clippy"]\n' > "$pinned/rust-toolchain.toml"
assert_resolves auto "$pinned" 1.95.0

# The legacy bare-name file is honored too.
legacy="$(workspace legacy)"
printf '1.90.0\n' > "$legacy/rust-toolchain"
assert_resolves auto "$legacy" 1.90.0

# A legacy file holding TOML is read as TOML.
legacy_toml="$(workspace legacy-toml)"
printf '[toolchain]\nchannel = "nightly-2026-01-01"\n' > "$legacy_toml/rust-toolchain"
assert_resolves auto "$legacy_toml" nightly-2026-01-01

# rust-toolchain.toml wins over the legacy file when both exist, matching rustup.
both="$(workspace both)"
printf '[toolchain]\nchannel = "1.95.0"\n' > "$both/rust-toolchain.toml"
printf '1.60.0\n' > "$both/rust-toolchain"
assert_resolves auto "$both" 1.95.0

# An explicit value still overrides the repository pin.
assert_resolves stable "$pinned" stable
assert_resolves 1.80.0 "$pinned" 1.80.0

# A pin this resolver cannot read fails loudly instead of silently installing something else.
unreadable="$(workspace unreadable)"
printf '[toolchain]\npath = "/opt/custom"\n' > "$unreadable/rust-toolchain.toml"
assert_fails auto "$unreadable" "set rust-toolchain: to an exact toolchain"

# An empty input is a configuration error, not an implicit default.
assert_fails "" "$unpinned" 'rust-toolchain must not be empty'

echo "action toolchain tests: OK"
