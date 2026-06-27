#!/usr/bin/env bash
# Publish gnr8 crates to crates.io.
#
# This is wired like exlint but intentionally opt-in in the release workflow. The GitHub release
# archives are the primary install path until cargo-install sidecar-resource behavior is finalized.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PACKAGES=(
  gnr8-core
  gnr8
)

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
  echo "DRY_RUN: smoke-check packaging for ${PACKAGES[*]}."
  cargo publish -p gnr8-core --dry-run --locked --allow-dirty

  core_version="$(crate_version gnr8-core)"
  if crate_version_exists gnr8-core "$core_version"; then
    cargo publish -p gnr8 --dry-run --locked --allow-dirty
  else
    echo "DRY_RUN: gnr8 depends on gnr8-core ${core_version}, which is not on crates.io yet."
    echo "DRY_RUN: checking gnr8 package contents; full publish dry-run is possible after gnr8-core is published."
    cargo package -p gnr8 --list --locked --allow-dirty >/dev/null
  fi

  printf '\nPublish: %s\n' "${PACKAGES[*]}"
  exit 0
fi

if [[ -z "${CRATES_IO_TOKEN:-}" && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  echo "error: set CRATES_IO_TOKEN or CARGO_REGISTRY_TOKEN" >&2
  exit 1
fi

if [[ -n "${CRATES_IO_TOKEN:-}" && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
  export CARGO_REGISTRY_TOKEN="$CRATES_IO_TOKEN"
fi

for package in "${PACKAGES[@]}"; do
  publish_crate "$package"
done

echo "Done. Verify: https://crates.io/crates/gnr8"

