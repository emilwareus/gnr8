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
project_root="$smoke_root/project"
mkdir -p "$install_root" "$project_root"
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
  "$binary" init --source fastapi --sdk python
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
  "$binary" generate
  if ! "$binary" --json doctor > doctor.json; then
    echo "archive doctor failed:" >&2
    cat doctor.json >&2
    exit 1
  fi
  "$binary" check
)

python3 - "$project_root/doctor.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    report = json.load(handle)
if not report.get("healthy"):
    raise SystemExit(f"archive doctor reported unhealthy: {report}")
PY

echo "archive smoke passed: init, generate, doctor, check"
