#!/usr/bin/env python3
"""Emit one SHA256 checksum line in GNU `sha256sum` text format."""

from __future__ import annotations

import argparse
import hashlib
import sys
from pathlib import Path


def checksum_line(path: Path) -> str:
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    return f"{digest}  {path.name}\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path, help="file to hash")
    parser.add_argument("-o", "--output", type=Path, default=None)
    args = parser.parse_args(argv)

    if not args.path.is_file():
        print(f"error: not a file: {args.path}", file=sys.stderr)
        return 1

    line = checksum_line(args.path).encode("utf-8")
    if args.output is None:
        # Bypass TextIO newline translation so Windows emits a portable LF-only
        # checksum file when stdout is redirected by the release script.
        sys.stdout.buffer.write(line)
    else:
        args.output.write_bytes(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
