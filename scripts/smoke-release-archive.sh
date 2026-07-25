#!/usr/bin/env bash
# Unpack a host release archive outside the checkout and exercise the installed lifecycle.
set -euo pipefail

archive="${1:-}"
if [[ -z "$archive" || ! -f "$archive" ]]; then
  echo "usage: scripts/smoke-release-archive.sh <gnr8-*.tar.gz>" >&2
  exit 1
fi
archive="$(cd "$(dirname "$archive")" && pwd)/$(basename "$archive")"

smoke_root="$(mktemp -d)"
trap 'rm -rf -- "$smoke_root"' EXIT
install_root="$smoke_root/install"
link_root="$smoke_root/local-bin"
project_root="$smoke_root/project"
mkdir -p "$install_root" "$link_root" "$project_root"
tar -xzf "$archive" -C "$install_root"

binary="$install_root/bin/gnr8"
if [[ ! -x "$binary" ]]; then
  echo "archive does not contain an executable bin/gnr8" >&2
  exit 1
fi
for required in \
  "$install_root/share/gnr8/Cargo.toml" \
  "$install_root/share/gnr8/crates/gnr8-core/Cargo.toml" \
  "$install_root/share/gnr8/crates/gnr8/Cargo.toml" \
  "$install_root/share/gnr8/pyextract/__main__.py"; do
  if [[ ! -f "$required" ]]; then
    echo "archive resource missing: $required" >&2
    exit 1
  fi
done

# Mimic the official installer: expose gnr8 through a PATH symlink, not the real binary path.
ln -sf "$binary" "$link_root/gnr8"
export PATH="$link_root:$PATH"
unset GNR8_RESOURCE_DIR || true

cat > "$project_root/app.py" <<'PY'
from fastapi import FastAPI
from pydantic import BaseModel

app = FastAPI()


class Book(BaseModel):
    title: str


@app.get("/books")
async def list_books() -> list[Book]:
    return []
PY

(
  cd "$project_root"
  # Invocation must go through the symlink with GNR8_RESOURCE_DIR unset.
  gnr8 --version
  gnr8 init --source fastapi --sdk python

  python3 - "$install_root/share/gnr8/crates/gnr8-core" .gnr8/Cargo.toml <<'PY'
from pathlib import Path
import re
import sys

core = Path(sys.argv[1]).resolve()
manifest = Path(sys.argv[2])
text = manifest.read_text(encoding="utf-8")

# Assert the shipped pin before rewriting it: a packaged `init` must emit an exact crates.io
# version pin, never a machine-local path. This is the contract the smoke test is here to check.
pin = re.search(r'^gnr8 = "=(\d+\.\d+\.\d+)"$', text, re.MULTILINE)
if not pin:
    raise SystemExit(
        f"packaged init must pin an exact crates.io version, got:\n{text}"
    )
print(f"packaged init pinned gnr8 =\"={pin.group(1)}\"")

# Remap to the archive's own crate so the rest of the lifecycle runs offline and against the
# binary under test, rather than whatever that version resolves to on crates.io.
text = text[: pin.start()] + f'gnr8 = {{ path = "{core}" }}' + text[pin.end() :]
manifest.write_text(text, encoding="utf-8")
PY

  python3 - .gnr8/src/main.rs <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
source = path.read_text(encoding="utf-8")
needle = 'PySdk::new().module('
if needle not in source:
    raise SystemExit("FastAPI/Python scaffold did not contain the expected PySdk target")
path.write_text(source.replace(needle, 'PySdk::new().dataclasses().module(', 1), encoding="utf-8")
PY
  gnr8 generate
  if ! gnr8 --json doctor > doctor.json; then
    echo "archive doctor failed:" >&2
    cat doctor.json >&2
    exit 1
  fi
  gnr8 check
)

python3 - "$project_root/doctor.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
if not report.get("healthy"):
    raise SystemExit(f"archive doctor reported unhealthy: {report}")
PY

echo "archive smoke passed: symlink install, init, generate, doctor, check"
