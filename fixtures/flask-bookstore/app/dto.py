"""Typed DTOs for the Flask bookstore fixture — the HONEST second-class envelope.

Flask is genuinely less typed than FastAPI: routing is decorator-based but the
request/response bodies are ordinary `dict`/`request.json` unless the author
OPTS IN to typed DTOs. This fixture encodes the opt-in typed envelope (PYSRC-02):
`@dataclass` request/response DTOs whose field types ARE the API facts, derived
from Python's own type system — never from a third-party schema-annotation tool
and never from a runtime schema export (CLAUDE.md rule 1).

Where Flask is genuinely untyped (a raw `request.json` read, a stringly-typed
query arg with no annotation), the Phase-2 extractor must emit a DIAGNOSTIC, not
guess a fact (rule 3, no fallback). Those untyped spots are marked in `routes.py`.

OPTIONAL vs NULLABLE axes (same two distinct axes as the FastAPI fixture):
  * optional = the JSON key may be absent (field has a default).
  * nullable = the value may be `None` (type admits `None`).
"""

from __future__ import annotations

import enum
from dataclasses import dataclass, field
from typing import Literal, Optional, Union


class Availability(str, enum.Enum):
    """A string enum (`enum.Enum`) -> neutral `Type::Enum` (members sorted).

    Declared out of lexical order so the snapshot proves the extractor sorts:
    [in_stock, out_of_stock] -> sorted.
    """

    OUT_OF_STOCK = "out_of_stock"
    IN_STOCK = "in_stock"


# A `Literal[...]` enum -> the SAME neutral `Type::Enum` shape (cross-language enum).
Currency = Literal["usd", "eur"]


@dataclass
class Price:
    """A nested DTO referenced by `OrderInput` -> a `$ref` to this schema.

      - amount   : required `float`             (neither)
      - currency : `Currency` Literal enum, required (neither)
    """

    amount: float
    currency: Currency


@dataclass
class OrderInput:
    """The typed request envelope for POST /orders/. Encodes ALL FOUR
    optional x nullable combinations distinctly.

      - book_id   : required `int`                       -> optional=F, nullable=F (neither)
      - quantity  : `int` with a default                 -> optional=T, nullable=F (optional only)
      - note      : `Optional[str]` (no default)         -> optional=F, nullable=T (nullable only)
      - coupon    : `Optional[str]` with a default       -> optional=T, nullable=T (both)
      - price     : nested `Price` $ref, required        (neither)
      - tags      : `list[str]` with a default           -> optional=T, nullable=F (array)
      - discount  : `Union[int, float]`, nullable+optional (both, a union)
    """

    book_id: int  # neither
    price: Price  # nested object $ref
    quantity: int = 1  # optional only
    note: Optional[str] = field(default=None)  # the type admits None...
    # both: default present (optional) AND type admits None (nullable)
    coupon: Optional[str] = None
    tags: list[str] = field(default_factory=list)  # optional array, not nullable
    # a union with a default None -> optional AND nullable
    discount: Optional[Union[int, float]] = None


# `note` above is declared `Optional[str]` WITH a default, so it is BOTH; to also
# cover the "nullable only" axis (value may be None, key required) we expose it on
# the response DTO below where it carries NO default.
@dataclass
class OrderConfirmation:
    """The typed response envelope for the order endpoints.

      - order_id     : required `int`                 (neither)
      - availability : `Availability` enum, required  (neither)
      - message      : `Optional[str]` (no default)   -> nullable only
      - lines        : `list[Price]` array of $refs   (neither)
    """

    order_id: int
    availability: Availability
    message: Optional[str]  # nullable only: value may be None, key required
    lines: list[Price]
