#!/usr/bin/env bash
# Build one gnr8 release archive in the same asset-name style as exlint.
#
# Required env:
#   TARGET      Rust target triple, e.g. x86_64-unknown-linux-gnu
#   ASSET_OS    linux | macos | windows
#   ASSET_ARCH  x86_64 | aarch64
#
# Optional env:
#   BINARY_NAME gnr8 or gnr8.exe (auto-detected from TARGET)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

target="${TARGET:-}"
asset_os="${ASSET_OS:-}"
asset_arch="${ASSET_ARCH:-}"

if [[ -z "$target" || -z "$asset_os" || -z "$asset_arch" ]]; then
  echo "usage: TARGET=<triple> ASSET_OS=<os> ASSET_ARCH=<arch> scripts/package-release.sh" >&2
  exit 1
fi

binary_name="${BINARY_NAME:-gnr8}"
case "$target" in
  *windows*) binary_name="${BINARY_NAME:-gnr8.exe}" ;;
esac

cargo build --locked --release -p gnr8-cli --target "$target"

binary_path="target/$target/release/$binary_name"
if [[ ! -f "$binary_path" ]]; then
  echo "built binary not found: $binary_path" >&2
  exit 1
fi

archive="gnr8-${asset_os}-${asset_arch}.tar.gz"
out="target/release-local-dist"
stage="$out/package"
dist="$out/dist"

rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/share/gnr8/crates" "$dist"

cp "$binary_path" "$stage/bin/"
if [[ "$binary_name" == "gnr8" ]]; then
  chmod 0755 "$stage/bin/$binary_name"
fi

cp README.md LICENSE Cargo.toml Cargo.lock rust-toolchain.toml "$stage/share/gnr8/"
cp -R crates/gnr8-core "$stage/share/gnr8/crates/"
cp -R goextract pyextract tsextract "$stage/share/gnr8/"

rm -rf \
  "$stage/share/gnr8/crates/gnr8-core/target" \
  "$stage/share/gnr8/tsextract/node_modules" \
  "$stage/share/gnr8/pyextract/__pycache__"
find "$stage/share/gnr8" -name '__pycache__' -type d -prune -exec rm -rf {} +

cat > "$stage/README.txt" <<EOF
gnr8 release archive

Add this archive's bin directory to PATH:
  export PATH="\$PWD/bin:\$PATH"

The share/gnr8 directory is required. gnr8 discovers it automatically from this archive layout, or you
can set GNR8_RESOURCE_DIR="\$PWD/share/gnr8" explicitly.

Runtime toolchains:
  - Rust/cargo: required because gnr8 compiles each project's .gnr8 generation crate.
  - Go: required for Go/Gin sources.
  - python3: required for FastAPI/Flask sources.
  - node + the target project's typescript dev dependency: required for NestJS sources.
EOF

tar -C "$stage" -czf "$dist/$archive" .
python3 "$ROOT/scripts/sha256_file.py" "$dist/$archive" > "$dist/$archive.sha256"
echo "$dist/$archive"
