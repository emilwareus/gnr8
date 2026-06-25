"""FastAPI bookstore service — STATIC fixture source (Phase 1).

Routes are declared with `@nestjs/common`'s Python analog — FastAPI's own
`@app`/`@router` decorators — and every request/response/param fact is derived
from the handler SIGNATURE + the typed models in `app.models`. Nothing here
reads FastAPI's runtime `/openapi.json` and nothing depends on a third-party
schema tool (CLAUDE.md rule 1). No app runs this phase (no `pip install`); this
is the static source the Phase-2 `pyextract` sidecar will read.

The routes mount under an `APIRouter(prefix="/books")` -> the neutral graph
operation paths are group-relative (`/`, `/{book_id}`); the `/books` prefix is
a lowering-time base path (rule 1: never folded into the code-derived path).
"""

from __future__ import annotations

from typing import Optional

from fastapi import APIRouter, FastAPI

from app.models import (
    Book,
    BookFilters,
    BookFormat,
    BookOrError,
    CreatedMessage,
    ListBooksResponse,
)

app = FastAPI(title="bookstore")
router = APIRouter(prefix="/books")


@router.get("/", response_model=ListBooksResponse)
def list_books(
    genre: str,
    sort: str = "asc",
    cursor: Optional[str] = None,
) -> ListBooksResponse:
    """GET /books/ — typed query params + a typed response envelope.

    Query params (derived from the signature):
      - genre  : required `str`        (no default)
      - sort   : optional `str`        (has a default)
      - cursor : optional `str`        (default None)
    Response: 200 -> `ListBooksResponse` ($ref).
    """
    raise NotImplementedError  # static fixture: never executed this phase


@router.post("/", response_model=CreatedMessage, status_code=201)
def create_book(book: Book) -> CreatedMessage:
    """POST /books/ — typed request body + a 201 typed response.

    Request body: `Book` ($ref). Response: 201 -> `CreatedMessage` ($ref).
    """
    raise NotImplementedError


@router.get("/{book_id}", response_model=BookOrError)
def get_book(book_id: int, fmt: Optional[BookFormat] = None) -> BookOrError:
    """GET /books/{book_id} — a path param + a UNION response.

    Params:
      - book_id : required path `int`
      - fmt     : optional query enum (`BookFormat`, default None)
    Response: 200 -> `BookOrError` (a union of `Book` and `OutOfStock`).
    """
    raise NotImplementedError


@router.put("/{book_id}", response_model=CreatedMessage)
def update_book(book_id: int, filters: BookFilters) -> CreatedMessage:
    """PUT /books/{book_id} — path param + a body exercising all four axes.

    Request body: `BookFilters` (the optional x nullable matrix).
    Response: 200 -> `CreatedMessage` ($ref).
    """
    raise NotImplementedError


app.include_router(router)
