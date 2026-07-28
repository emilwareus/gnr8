#!/usr/bin/env python3
"""Bump the release version in root Cargo.toml.

Updates [workspace.package] version and the public gnr8 workspace dependency version. Prints the
new version on stdout.

The bump level is an explicit operator choice, because Cargo treats 0.x.y -> 0.x.(y+1) as a
SEMVER-COMPATIBLE upgrade: a release that removes public API must move the minor, or every
downstream `gnr8 = "0.1"` picks up a build-breaking version silently.

    bump-workspace-version.py                # patch: 0.1.23 -> 0.1.24
    bump-workspace-version.py --minor        # minor: 0.1.23 -> 0.2.0
    bump-workspace-version.py --dry-run      # print the next version, change nothing
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def bump_release(version: str, *, minor: bool = False) -> str:
    parts = version.strip().split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        raise ValueError(f"expected VERSION like 0.1.0, got {version!r}")
    major, minor_part, patch = (int(parts[0]), int(parts[1]), int(parts[2]))
    if minor:
        return f"{major}.{minor_part + 1}.0"
    if major == 0 and minor_part == 0:
        return "0.1.0"
    return f"{major}.{minor_part}.{patch + 1}"


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Bump the release version in root Cargo.toml.",
    )
    parser.add_argument(
        "--minor",
        action="store_true",
        help="bump the minor version (required when the release removes or changes public API)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="print the version that would be written and exit without modifying anything",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parent.parent
    cargo_path = root / "Cargo.toml"
    text = cargo_path.read_text(encoding="utf-8")

    match = re.search(
        r"(?ms)^\[workspace\.package\].*?^version = \"([^\"]+)\"",
        text,
    )
    if not match:
        print("error: could not find [workspace.package] version", file=sys.stderr)
        sys.exit(1)

    current = match.group(1)
    new_ver = bump_release(current, minor=args.minor)

    if args.dry_run:
        print(new_ver, end="")
        return

    def replace_workspace_package_block(match: re.Match[str]) -> str:
        block = match.group(0)
        return re.sub(
            r"^version = \"[^\"]+\"",
            f'version = "{new_ver}"',
            block,
            count=1,
            flags=re.MULTILINE,
        )

    text, count = re.subn(
        r"(?ms)^\[workspace\.package\].*?(?=^\[|\Z)",
        replace_workspace_package_block,
        text,
        count=1,
    )
    if count != 1:
        print("error: failed to replace [workspace.package] block", file=sys.stderr)
        sys.exit(1)

    text, count = re.subn(
        r'^(gnr8 = \{ path = "crates/gnr8-core", version = ")[^"]+("\s*\})',
        rf"\g<1>{new_ver}\2",
        text,
        count=1,
        flags=re.MULTILINE,
    )
    if count != 1:
        print("error: expected exactly one gnr8 workspace dependency version", file=sys.stderr)
        sys.exit(1)

    cargo_path.write_text(text, encoding="utf-8")
    print(new_ver, end="")


if __name__ == "__main__":
    main()
