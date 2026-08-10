#!/usr/bin/env python3
"""Enforce the repository's hard five-minute GitHub Actions budget."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"
MAX_MINUTES = 5
FORBIDDEN_COMMANDS = (
    (
        re.compile(r"^(?!\s*name:).*\bmake check\b", re.MULTILINE),
        "make check",
    ),
    (re.compile(r"release-local-check\.sh"), "release-local-check.sh"),
)


def timeout_value(lines: list[str], indent: int) -> int | None:
    pattern = re.compile(
        rf"^{re.escape(' ' * indent)}timeout-minutes:\s*(\d+)\s*(?:#.*)?$"
    )
    for line in lines:
        match = pattern.match(line)
        if match:
            return int(match.group(1))
    return None


def blocks(lines: list[str], start_pattern: str) -> list[tuple[str, list[str]]]:
    starts: list[tuple[int, str]] = []
    pattern = re.compile(start_pattern)
    for index, line in enumerate(lines):
        match = pattern.match(line)
        if match:
            starts.append((index, match.group(1)))

    result: list[tuple[str, list[str]]] = []
    for position, (start, name) in enumerate(starts):
        end = starts[position + 1][0] if position + 1 < len(starts) else len(lines)
        block = lines[start:end]
        while block and block[-1].strip() == "":
            block.pop()
        result.append((name, block))
    return result


def check_workflow(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    errors: list[str] = []

    jobs_marker = next((index for index, line in enumerate(lines) if line == "jobs:"), None)
    if jobs_marker is None:
        return [f"{path.relative_to(ROOT)}: missing jobs section"]

    job_lines = lines[jobs_marker + 1 :]
    for job_name, job in blocks(job_lines, r"^  ([A-Za-z0-9_-]+):\s*$"):
        value = timeout_value(job, 4)
        if value is None:
            errors.append(f"{path.relative_to(ROOT)}: job {job_name!r} has no timeout-minutes")
        elif value > MAX_MINUTES:
            errors.append(
                f"{path.relative_to(ROOT)}: job {job_name!r} allows {value} minutes"
            )

    for pattern, command in FORBIDDEN_COMMANDS:
        if pattern.search(text):
            errors.append(
                f"{path.relative_to(ROOT)}: monolithic CI command is forbidden: {command!r}"
            )
    return errors


def main() -> int:
    errors: list[str] = []
    paths = sorted({*WORKFLOWS.glob("*.yml"), *WORKFLOWS.glob("*.yaml")})
    if not paths:
        print("CI budget violations: no workflow files found", file=sys.stderr)
        return 1
    for path in paths:
        errors.extend(check_workflow(path))
    if errors:
        print("CI budget violations:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print(f"CI budget: every job is capped at {MAX_MINUTES} minutes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
