"""Loader: discover every ``*.py`` under the target tree and ``ast.parse`` each.

This is the Python twin of ``goextract/internal/load/load.go`` — but it mirrors
only the *shape* (a ``load(target)`` returning structured parsed modules + per-file
parse errors as diagnostics, never an abort). It deliberately does NOT mirror that
file's ``golang.org/x/tools/go/packages`` dependency (rule-2 debt): the only thing
used here is the CPython standard library.

CRITICAL static-only boundary (threat T-static-exec / PYSRC-03): the loader reads
each file as TEXT and calls ONLY ``ast.parse``. It NEVER executes, imports, or
dynamically loads the target by any means. ``ast.parse`` builds a syntax tree
without executing any of it — that is the load-bearing security invariant of the
sidecar.
"""

import ast
import os


class Module:
    """One parsed target module.

    Attributes:
        dotted: the import-dotted module id relative to the target root, e.g.
            ``app.models`` (relpath with ``os.sep`` -> ``.`` and ``.py`` dropped).
        tree: the ``ast.Module`` produced by ``ast.parse`` (NOT executed).
        abs_path: the canonical absolute path of the source file (spans emit this;
            the host relativizes against the module root — Pattern C).
    """

    __slots__ = ("dotted", "tree", "abs_path")

    def __init__(self, dotted, tree, abs_path):
        self.dotted = dotted
        self.tree = tree
        self.abs_path = abs_path


def _dotted_module(rel_path):
    """Convert a target-relative ``*.py`` path into its import-dotted module id.

    ``app/models.py`` -> ``app.models``; ``app/__init__.py`` -> ``app`` (the
    package marker collapses to its directory). Always uses ``/`` semantics by
    normalizing ``os.sep`` first so output is identical across platforms.
    """
    without_ext = rel_path[: -len(".py")] if rel_path.endswith(".py") else rel_path
    dotted = without_ext.replace(os.sep, ".").replace("/", ".")
    if dotted.endswith(".__init__"):
        dotted = dotted[: -len(".__init__")]
    elif dotted == "__init__":
        dotted = ""
    return dotted


def discover(target_dir):
    """Return the sorted list of ``*.py`` absolute paths under ``target_dir``.

    Sorted by relative path for determinism (the host re-sorts the final facts, but
    the loader stays internally deterministic — Pattern B).
    """
    found = []
    for root, dirs, files in os.walk(target_dir):
        dirs.sort()
        for name in files:
            if name.endswith(".py"):
                found.append(os.path.join(root, name))
    found.sort()
    return found


def load(target_dir, diags):
    """Parse every ``*.py`` under ``target_dir`` statically.

    Returns a list of :class:`Module` in sorted dotted-id order. A ``SyntaxError``
    (or any read/parse failure) on a single file becomes a WARN diagnostic and is
    skipped — it NEVER aborts the run (mirrors goextract turning per-package load
    errors into diagnostics, GO-06).

    Args:
        target_dir: the canonical absolute target directory.
        diags: a diagnostics accumulator exposing ``warn(message, file, line)``.
    """
    modules = []
    for abs_path in discover(target_dir):
        rel_path = os.path.relpath(abs_path, target_dir)
        dotted = _dotted_module(rel_path)
        try:
            with open(abs_path, "r", encoding="utf-8") as handle:
                source = handle.read()
        except OSError as exc:
            diags.warn(
                "could not read source file: {}".format(exc), abs_path, 0
            )
            continue
        try:
            # STATIC ONLY: parse the file TEXT. ast.parse does not execute the
            # target. type_comments stays off; we never need the runtime types.
            tree = ast.parse(source, filename=abs_path)
        except SyntaxError as exc:
            line = exc.lineno if exc.lineno is not None else 0
            diags.warn(
                "could not parse Python source: {}".format(exc.msg),
                abs_path,
                line,
            )
            continue
        modules.append(Module(dotted, tree, abs_path))
    modules.sort(key=lambda m: m.dotted)
    return modules
