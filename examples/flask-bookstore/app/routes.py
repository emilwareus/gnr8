"""Flask bookstore routes — STATIC fixture source (Phase 1).

Routing uses a Flask `Blueprint` with a static URL prefix. The extractor composes
that `/orders` prefix into each neutral-graph operation path. Path params use
Flask's `<int:order_id>` converter.

The HONEST envelope (PYSRC-02): typed handlers -> facts; raw `request.json` /
unannotated `request.args.get(...)` -> a DIAGNOSTIC, never a guess (rule 3).

No app runs this phase (no `pip install`); this is the static source pyextract reads.

The blank lines / comments below carry NO API fact (rule 1): they exist only to land
each AST anchor on the line the committed snapshot asserts — every fact still derives
purely from the decorator Call, the typed signature, and the annotated body reads.
This is the honest second-class envelope: Flask bodies are plain `request.json`
unless the author opts in to a typed DTO, so the genuinely untyped spots become
diagnostics (the limit is visible in `create_order_raw` below).
"""

from __future__ import annotations

from flask import Blueprint, request

from app.dto import OrderConfirmation, OrderInput

bp = Blueprint("orders", __name__, url_prefix="/orders")


@bp.route("/", methods=["GET"])
def list_orders() -> OrderConfirmation:
    """GET /orders/ — a typed response; one typed + one UNTYPED query param.

    Query params:
      - status : read via the framework's typed query helper (typed) -> a fact.
      - q      : raw stringly-typed read with no annotation -> UNTYPED, diagnostic.

    Response: 200 -> `OrderConfirmation` ($ref). The status is method-derived
    (GET -> 200), a code fact; the docstring is never read for it (rule 1).
    """
    status: str = request.args.get("status", "in_stock")  # typed query -> fact
    q = request.args.get("q")  # UNTYPED -> diagnostic in Phase 2 (no annotation)
    _ = (status, q)
    raise NotImplementedError  # static fixture: never executed this phase


@bp.route("/", methods=["POST"])
def create_order() -> OrderConfirmation:
    """POST /orders/ — a typed request DTO body + a typed response.

    Body: `OrderInput`; status method-derived (POST -> 201), a code fact (OQ1).
    """
    order: OrderInput = OrderInput(**request.json)  # typed DTO body -> fact
    _ = order
    raise NotImplementedError


@bp.route("/<int:order_id>", methods=["GET"])
def get_order(order_id: int) -> OrderConfirmation:
    """GET /orders/<int:order_id> — an `<int:...>` converter path param.

    Params: order_id : required path `int` (from the `<int:...>` converter).
    Response: 200 -> `OrderConfirmation` ($ref).
    """
    raise NotImplementedError


@bp.route("/raw", methods=["POST"])
def create_order_raw():
    """POST /orders/raw — the HONEST untyped envelope.

    The body is read straight from `request.json` with NO typed DTO and NO return
    annotation, so neither the request body nor the response is a source fact: the
    extractor emits a DIAGNOSTIC, and `.gnr8` explicitly declares the bodyless
    `201` response (rule 3, no guessing). The extra
    non-fact prose here (rule 1) only positions the `request.json` read on the
    snapshot's asserted line; it encodes nothing about the API surface itself.
    """
    payload = request.json  # UNTYPED -> diagnostic in Phase 2 (no DTO)
    _ = payload
    raise NotImplementedError
