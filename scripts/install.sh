#!/usr/bin/env bash
set -euo pipefail

repo="${GNR8_REPO:-emilwareus/gnr8}"
tag="${GNR8_RELEASE_TAG:-latest}"
install_root="${GNR8_INSTALL_ROOT:-$HOME/.local/gnr8}"
bin_dir="${GNR8_BIN_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
  Darwin) os="macos" ;;
  Linux) os="linux" ;;
  *)
    echo "gnr8 install: unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *)
    echo "gnr8 install: unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

asset="gnr8-${os}-${arch}.tar.gz"
if [[ "${tag}" == "latest" ]]; then
  base_url="https://github.com/${repo}/releases/latest/download"
else
  base_url="https://github.com/${repo}/releases/download/${tag}"
fi

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -q "$1" -O "$2"; }
else
  echo "gnr8 install: curl or wget is required to download release assets" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

echo "Downloading ${asset} from ${repo} release ${tag}..."
fetch "${base_url}/${asset}" "${tmp_dir}/${asset}"
fetch "${base_url}/${asset}.sha256" "${tmp_dir}/${asset}.sha256"

(
  cd "$tmp_dir"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "${asset}.sha256"
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "${asset}.sha256"
  else
    echo "gnr8 install: shasum or sha256sum is required to verify ${asset}" >&2
    exit 1
  fi
  tar -xzf "$asset"
)

rm -rf "$install_root"
mkdir -p "$install_root" "$bin_dir"
cp -R "$tmp_dir"/. "$install_root"/
ln -sfn "$install_root/bin/gnr8" "$bin_dir/gnr8"

echo "Installed gnr8 to ${install_root}"
echo "Linked ${bin_dir}/gnr8"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *)
    echo "Note: ${bin_dir} is not on PATH. Add it to your shell profile to run gnr8 directly."
    ;;
esac

"$bin_dir/gnr8" --version

