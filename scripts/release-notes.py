#!/usr/bin/env python3
"""Validate a dated changelog section and render it as GitHub Release notes."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CHANGELOG = ROOT / "CHANGELOG.md"
VERSION_PATTERN = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
DATE_PATTERN = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$")
H2_PATTERN = re.compile(r"^## (.+?)\s*$", re.MULTILINE)


class ChangelogError(ValueError):
    """The changelog is not ready to publish."""


@dataclass(frozen=True)
class ReleaseSection:
    heading: str
    date: str
    content: str


def release_section(text: str, version: str) -> ReleaseSection:
    if not VERSION_PATTERN.fullmatch(version):
        raise ChangelogError(f"expected VERSION like 0.5.0, got {version!r}")

    headings = list(H2_PATTERN.finditer(text))
    unreleased = [match for match in headings if match.group(1) == "Unreleased"]
    if len(unreleased) != 1:
        raise ChangelogError("expected exactly one '## Unreleased' section")

    unreleased_match = unreleased[0]
    unreleased_index = headings.index(unreleased_match)
    unreleased_end = (
        headings[unreleased_index + 1].start()
        if unreleased_index + 1 < len(headings)
        else len(text)
    )
    if text[unreleased_match.end() : unreleased_end].strip():
        raise ChangelogError(
            "'## Unreleased' must be empty before publishing; move its entries into the dated "
            f"{version} section"
        )

    prefix = f"{version} — "
    releases = [match for match in headings if match.group(1).startswith(prefix)]
    if len(releases) != 1:
        raise ChangelogError(
            f"expected exactly one '## {version} — YYYY-MM-DD' section"
        )

    release_match = releases[0]
    date = release_match.group(1)[len(prefix) :]
    if not DATE_PATTERN.fullmatch(date):
        raise ChangelogError(
            f"release heading must be '## {version} — YYYY-MM-DD', got {release_match.group(1)!r}"
        )

    release_index = headings.index(release_match)
    release_end = (
        headings[release_index + 1].start()
        if release_index + 1 < len(headings)
        else len(text)
    )
    content = text[release_match.end() : release_end].strip()
    if not content:
        raise ChangelogError(f"the {version} changelog section is empty")

    return ReleaseSection(heading=release_match.group(1), date=date, content=content)


def github_anchor(section: ReleaseSection) -> str:
    version = section.heading.split(" — ", maxsplit=1)[0].replace(".", "")
    return f"{version}--{section.date}"


def render_body(section: ReleaseSection, repo: str) -> str:
    if not re.fullmatch(r"[^/\s]+/[^/\s]+", repo):
        raise ChangelogError(f"expected REPO like owner/name, got {repo!r}")

    install = (
        "Install with the archive assets below, or run: "
        f"`curl -fsSL https://raw.githubusercontent.com/{repo}/main/scripts/install.sh | bash`."
    )
    changelog_url = (
        f"https://github.com/{repo}/blob/main/CHANGELOG.md#{github_anchor(section)}"
    )
    return (
        f"{install}\n\n"
        f"## {section.heading}\n\n{section.content}\n\n"
        f"[View this release in the changelog]({changelog_url}).\n"
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate and render a versioned CHANGELOG.md section.",
    )
    parser.add_argument("command", choices=("check", "body"))
    parser.add_argument("version")
    parser.add_argument("--changelog", type=Path, default=DEFAULT_CHANGELOG)
    parser.add_argument("--repo", help="GitHub owner/repository, required for 'body'")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        text = args.changelog.read_text(encoding="utf-8")
        section = release_section(text, args.version)
        if args.command == "body":
            if args.repo is None:
                raise ChangelogError("--repo is required for 'body'")
            print(render_body(section, args.repo), end="")
        else:
            print(f"changelog: {section.heading} is ready")
    except (ChangelogError, OSError) as error:
        print(f"release notes error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
