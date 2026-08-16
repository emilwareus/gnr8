#!/usr/bin/env bash
# Resolve which Rust toolchain the gnr8 check action installs.
#
# The `.gnr8` crate is compiled by CI and by every developer machine, so the two must agree. A
# repository that pins a toolchain has already stated which one it wants; installing a different
# channel on top of that pin is a silent source of divergence. "auto" therefore honors the pin and
# falls back to "stable" only when the repository states nothing. Any explicit value overrides.
set -euo pipefail

# Unset means the resolver was run outside the action; empty means the caller passed an empty input,
# which is a configuration error rather than an implicit default.
requested_toolchain="${REQUESTED_TOOLCHAIN-auto}"
workspace="${GITHUB_WORKSPACE:-.}"

emit() {
  echo "toolchain=$1" >> "${GITHUB_OUTPUT:-/dev/stdout}"
}

if [[ -z "$requested_toolchain" ]]; then
  echo 'gnr8 action: rust-toolchain must not be empty; use "auto" or an exact toolchain' >&2
  exit 2
fi

if [[ "$requested_toolchain" != "auto" ]]; then
  echo "gnr8 action: installing the requested Rust toolchain \"$requested_toolchain\""
  emit "$requested_toolchain"
  exit 0
fi

pin=""
for candidate in rust-toolchain.toml rust-toolchain; do
  if [[ -f "$workspace/$candidate" ]]; then
    pin="$workspace/$candidate"
    break
  fi
done

if [[ -z "$pin" ]]; then
  echo 'gnr8 action: no repository Rust toolchain pin; installing "stable"'
  emit stable
  exit 0
fi

channel="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$pin" | head -n 1)"
if [[ -z "$channel" && "$(basename "$pin")" == "rust-toolchain" ]]; then
  # The legacy file is a bare toolchain name rather than TOML.
  channel="$(tr -d '[:space:]' < "$pin")"
fi
if [[ -z "$channel" ]]; then
  echo "gnr8 action: $pin pins a toolchain this action cannot read; set rust-toolchain: to an exact toolchain" >&2
  exit 2
fi

echo "gnr8 action: honoring $pin (toolchain \"$channel\")"
emit "$channel"
