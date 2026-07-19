"""FastAPI bookstore service — STATIC fixture source (Phase 1).

Routes are declared with FastAPI's own `@app`/`@router` decorators, and every
request/response/param fact is derived from the handler SIGNATURE + the typed
models in `app.models`. Nothing here reads FastAPI's runtime `/openapi.json` and
nothing depends on a third-party schema tool (CLAUDE.md rule 1). No app runs this
phase (no `pip install`); this is the static source `pyextract` reads.

The routes mount under an `APIRouter(prefix="/books")`; that static framework
prefix is composed into every operation path (`/books/`, `/books/{book_id}`).
An external service mount remains a separate code-configured base path.

NOTE ON LINE LAYOUT (RESEARCH OQ2): the committed FastAPI graph snapshot pins
each route span to the handler `def` line and each param span to the param's own
signature line; the layout is spaced so each anchor lands on the asserted line.
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
    (Layout note: exercises the typed-query-param path.)
    """
    raise NotImplementedError  # static fixture: never executed this phase


# create_book registers POST with a typed body and a 201 status_code anchor.
@router.post("/", response_model=CreatedMessage, status_code=201)
def create_book(book: Book) -> CreatedMessage:
    """POST /books/ — typed request body + a 201 typed response.
    Request body: `Book` ($ref). Response: 201 -> `CreatedMessage` ($ref).
    """
    raise NotImplementedError


@router.get("/{book_id}", response_model=BookOrError)
def get_book(
    book_id: int, fmt: Optional[BookFormat] = None
) -> BookOrError:
    """GET /books/{book_id} — a path param + a UNION response.
    Response: 200 -> `BookOrError` (a union of `Book` and `OutOfStock`).
    """
    raise NotImplementedError


@router.put("/{book_id}", response_model=CreatedMessage)
def update_book(
    book_id: int, filters: BookFilters
) -> CreatedMessage:
    """PUT /books/{book_id} — path param + a body exercising all four axes.

    Request body: `BookFilters` (the optional x nullable matrix).
    Response: 200 -> `CreatedMessage` ($ref).
    """
    raise NotImplementedError


app.include_router(router)
