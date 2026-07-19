#!/usr/bin/env bash
# Local dry-run of release pieces, mirroring `.github/workflows/release*.yml`.
#
# Usage:
#   ./scripts/release-local-check.sh
#   WITH_LINUX_AARCH64=1 ./scripts/release-local-check.sh
#   DRY_RUN=0 CRATES_IO_TOKEN=... ./scripts/release-local-check.sh   # real publish (careful)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> make check"
make check

host_target="$(rustc -vV | sed -n 's/^host: //p')"
case "$host_target" in
  x86_64-apple-darwin)
    asset_os=macos
    asset_arch=x86_64
    ;;
  aarch64-apple-darwin)
    asset_os=macos
    asset_arch=aarch64
    ;;
  x86_64-unknown-linux-gnu)
    asset_os=linux
    asset_arch=x86_64
    ;;
  *)
    echo "!!! Skipping host package: unsupported local host target ${host_target}."
    asset_os=
    asset_arch=
    ;;
esac

if [[ -n "${asset_os:-}" ]]; then
  echo "==> package host archive: ${asset_os}-${asset_arch}"
  TARGET="$host_target" ASSET_OS="$asset_os" ASSET_ARCH="$asset_arch" scripts/package-release.sh
  echo "==> smoke unpacked host archive"
  scripts/smoke-release-archive.sh \
    "target/release-local-dist/dist/gnr8-${asset_os}-${asset_arch}.tar.gz"
fi

if [[ "${WITH_LINUX_AARCH64:-}" == "1" ]]; then
  if ! command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
    echo "!!! Skipping aarch64-unknown-linux-gnu build: no aarch64-linux-gnu-gcc."
  else
    rustup target add aarch64-unknown-linux-gnu 2>/dev/null || true
    TARGET=aarch64-unknown-linux-gnu ASSET_OS=linux ASSET_ARCH=aarch64 scripts/package-release.sh
  fi
fi

echo "==> cargo publish --dry-run (no upload)"
DRY_RUN=1 ./scripts/publish-crates.sh

if [[ "${DRY_RUN:-1}" == "0" || "${DRY_RUN:-1}" == "false" ]]; then
  if [[ -z "${CRATES_IO_TOKEN:-}" && -z "${CARGO_REGISTRY_TOKEN:-}" ]]; then
    echo "error: set CRATES_IO_TOKEN or CARGO_REGISTRY_TOKEN for real publish" >&2
    exit 1
  fi
  echo "==> cargo publish (REAL)"
  ./scripts/publish-crates.sh
else
  echo "==> skip real publish (set DRY_RUN=0 CRATES_IO_TOKEN=... to publish)"
fi

echo "OK release-local-check complete."
