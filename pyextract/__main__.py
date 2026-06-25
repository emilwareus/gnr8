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
import traceback

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
    # Parallel deterministic recognizers (Plan 03 FastAPI + Plan 04 Flask). Each one
    # keys off its OWN router-construct NAMES (rule 1): the FastAPI recognizer fires
    # only on a tree with FastAPI()/APIRouter() bindings, the Flask recognizer only on
    # Flask()/Blueprint() bindings. A tree of one shape yields an empty list from the
    # other — detection by source shape, NOT a try-A-then-fall-back-to-B path (rule 3).
    routes = routes_mod.recognize_fastapi(modules, symtab, diags)
    routes += routes_mod.recognize_flask(modules, symtab, diags)

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
        # Emit the one-line identity AND the full traceback to stderr so a genuine
        # internal bug (an unhandled AST shape -> AttributeError/KeyError) is
        # diagnosable, not masked as a clean tool diagnostic (WR-07). stdout stays
        # reserved for facts JSON; the non-zero exit still maps to HelperExit.
        sys.stderr.write("pyextract: {}\n".format(exc))
        sys.stderr.write(traceback.format_exc())
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
