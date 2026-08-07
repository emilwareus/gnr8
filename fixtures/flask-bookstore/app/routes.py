"""Flask bookstore routes — STATIC fixture source (Phase 1).

Routing uses Flask `Blueprint`s with a URL prefix (the blueprint-prefix shape):
the static `/orders` prefix is composed into each neutral operation path. Path
params use Flask's `<int:order_id>` converter.


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
    """List orders, optionally narrowed to one stock status.

    Orders are returned newest first. Omit the status filter to list every order
    regardless of availability.
    """
    # Fixture notes live in the BODY, never in the docstring: only the docstring
    # becomes API prose, and the response status is method-derived (GET -> 200) as a
    # code fact that no docstring can state or override.
    #   status : typed query helper read -> a fact.
    #   q      : raw stringly-typed read, no annotation -> UNTYPED, diagnostic.
    status: str = request.args.get("status", "in_stock")  # typed query -> fact
    q = request.args.get("q")  # UNTYPED -> diagnostic in Phase 2 (no annotation)
    _ = (status, q)
    raise NotImplementedError  # static fixture: never executed this phase


@bp.route("/", methods=["POST"])
def create_order() -> OrderConfirmation:
    """Place a new order.

    The order is confirmed immediately and its confirmation number is returned.
    """
    # Fixture note: typed DTO body -> a fact; status method-derived (POST -> 201).
    order: OrderInput = OrderInput(**request.json)  # typed DTO body -> fact
    _ = order
    raise NotImplementedError


@bp.route("/<int:order_id>", methods=["GET"])
def get_order(order_id: int) -> OrderConfirmation:
    """Fetch one order by its identifier."""
    # Fixture note: exercises the `<int:...>` converter path param.
    raise NotImplementedError


@bp.route("/raw", methods=["POST"])
def create_order_raw():
    """Place an order from a raw, unvalidated payload.

    Provided for clients that cannot produce the typed order shape. The payload is
    accepted as-is and validated downstream.
    """
    # Fixture note, deliberately in the BODY: the body is read straight from
    # `request.json` with NO typed DTO and NO return annotation, so neither the
    # request body nor the response is a fact and the extractor must emit a
    # DIAGNOSTIC (rule 3, no guessing). The docstring above is ordinary API prose
    # and encodes nothing about the API surface — proving prose and structure are
    # genuinely independent.
    payload = request.json  # UNTYPED -> diagnostic in Phase 2 (no DTO)
    _ = payload
    raise NotImplementedError
