#!/usr/bin/env bash
# Publish the public gnr8 crate to crates.io.
#
# This is wired like exlint but intentionally opt-in in the release workflow. The GitHub release
# archives are the primary install path until cargo-install sidecar-resource behavior is finalized.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PACKAGE=gnr8

crate_version() {
  cargo pkgid -p "$1" | sed 's/.*#//'
}

crate_version_exists() {
  local name="$1"
  local version="$2"
  local tmp_dir
  local status

  tmp_dir="$(mktemp -d)"
  (
    cd "$tmp_dir"
    cargo info "${name}@${version}" --registry crates-io >/dev/null 2>&1
  )
  status=$?
  rm -rf "$tmp_dir"
  return "$status"
}

wait_for_crate_version() {
  local name="$1"
  local version="$2"

  for _ in {1..30}; do
    if crate_version_exists "$name" "$version"; then
      return 0
    fi
    sleep 10
  done

  echo "error: ${name} ${version} was not visible in Cargo registry metadata after publishing" >&2
  return 1
}

publish_crate() {
  local name="$1"
  local version

  version="$(crate_version "$name")"
  if crate_version_exists "$name" "$version"; then
    echo "Skipping ${name} ${version}; already published."
    return 0
  fi

  echo "Publishing ${name}..."
  cargo publish -p "$name" --locked
  wait_for_crate_version "$name" "$version"
}

if [[ "${DRY_RUN:-}" == "1" || "${DRY_RUN:-}" == "true" ]]; then
  echo "DRY_RUN: smoke-check packaging for ${PACKAGE}."
  cargo publish -p "$PACKAGE" --dry-run --locked --allow-dirty

  printf '\nPublish: %s\n' "$PACKAGE"
  exit 0
fi

if [[ -z "${CRATES_IO_TOKEN:-}" && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "error: set CRATES_IO_TOKEN or CARGO_REGISTRY_TOKEN" >&2
  exit 1
fi

if [[ -n "${CRATES_IO_TOKEN:-}" && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  export CARGO_REGISTRY_TOKEN="$CRATES_IO_TOKEN"
fi

publish_crate "$PACKAGE"

echo "Done. Verify: https://crates.io/crates/gnr8"
