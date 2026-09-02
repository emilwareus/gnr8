from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("release-notes.py")
SPEC = importlib.util.spec_from_file_location("release_notes", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
release_notes = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release_notes
SPEC.loader.exec_module(release_notes)


class ReleaseNotesTests(unittest.TestCase):
    def test_accepts_empty_unreleased_and_renders_dated_section(self) -> None:
        text = """# Changelog

## Unreleased

## 0.5.0 — 2026-08-10

### Added

- Safe output adoption.

## 0.4.0 — 2026-07-01

- Earlier release.
"""

        section = release_notes.release_section(text, "0.5.0")
        body = release_notes.render_body(section, "oaiz-io/gnr8")

        self.assertEqual(section.date, "2026-08-10")
        self.assertIn("### Added\n\n- Safe output adoption.", body)
        self.assertIn("/CHANGELOG.md#050--2026-08-10", body)
        self.assertNotIn("Earlier release", body)

    def test_rejects_changes_left_under_unreleased(self) -> None:
        text = """# Changelog

## Unreleased

### Fixed

- Still waiting to be released.

## 0.5.0 — 2026-08-10

- Released.
"""

        with self.assertRaisesRegex(release_notes.ChangelogError, "must be empty"):
            release_notes.release_section(text, "0.5.0")

    def test_rejects_missing_version_section(self) -> None:
        text = """# Changelog

## Unreleased

## 0.4.0 — 2026-07-01

- Earlier release.
"""

        with self.assertRaisesRegex(release_notes.ChangelogError, "0.5.0"):
            release_notes.release_section(text, "0.5.0")

    def test_rejects_undated_version_section(self) -> None:
        text = """# Changelog

## Unreleased

## 0.5.0

- Released without a date.
"""

        with self.assertRaisesRegex(release_notes.ChangelogError, "YYYY-MM-DD"):
            release_notes.release_section(text, "0.5.0")


if __name__ == "__main__":
    unittest.main()
