"""Entrypoint: ``python3 -m pyextract <target-dir>`` — argv -> facts JSON on stdout.

Mirrors ``goextract/main.go``: a single target-dir argument, the facts JSON written
to ``stdout`` ONLY, and every tool-diagnostic-about-itself written to ``stderr`` with
a non-zero exit. The Rust subprocess driver maps a non-zero exit to ``HelperExit``
and unparsable stdout to ``FactsParse`` — so the contract is strict: **stdout is the
facts JSON and nothing else**.

``run()`` orders the pipeline exactly like ``goextract``'s ``run``:
``load -> diagnostics -> symbol table -> schemas -> module basename -> routes (empty
this plan) -> assemble -> marshal``.
"""

import os
import sys

from pyextract import facts, load, routes as routes_mod
from pyextract.diagnostics import Diagnostics
from pyextract.schemas import build_schemas
from pyextract.symtab import SymbolTable


def run(target_dir):
    """Build the neutral facts JSON string for ``target_dir``.

    Args:
        target_dir: the canonical (already ``os.path.realpath``'d) target directory.

    Returns:
        A deterministic, sorted, byte-stable JSON facts document string.
    """
    diags = Diagnostics()
    modules = load.load(target_dir, diags)
    symtab = SymbolTable(modules)

    schemas = build_schemas(modules, symtab, diags)

    module = os.path.basename(target_dir)
    # FastAPI route recognition (Plan 03). Flask lands in Plan 04. The recognizer
    # emits nothing for a tree with no router/app bindings, so a non-FastAPI tree
    # yields an empty routes list deterministically (no fallback).
    routes = routes_mod.recognize_fastapi(modules, symtab, diags)

    doc = facts.build_doc(module, routes, schemas, diags.items())
    return facts.marshal(doc)


def main(argv):
    """CLI guard + orchestration. Returns the process exit code."""
    if len(argv) < 2:
        sys.stderr.write("usage: python3 -m pyextract <target-dir>\n")
        return 1
    target_dir = os.path.realpath(argv[1])
    try:
        sys.stdout.write(run(target_dir))
        sys.stdout.write("\n")
    except Exception as exc:  # noqa: BLE001 — surface ANY failure to stderr, exit 1
        sys.stderr.write("pyextract: {}\n".format(exc))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
