from __future__ import annotations

import dataclasses
import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Optional

from .errors import ApiError
from .models import *  # noqa: F401,F403  (re-export models for return-type annotations)


class Client:
    """Dependency-free SDK client over urllib (no requests/httpx)."""

    def __init__(
        self,
        base_url: str,
        *,
        api_key: Optional[str] = None,
        opener: Optional[urllib.request.OpenerDirector] = None,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._opener = opener or urllib.request.build_opener()

    def _do(self, method: str, path: str, *, body: Optional[Any] = None) -> tuple:
        # A typed request-body model is a @dataclass, which json.dumps cannot serialize directly
        # (TypeError) — marshal it to a dict first (dataclasses.asdict recurses into nested
        # dataclasses). The single deterministic encode path; stdlib only (CLAUDE.md rule 2).
        if body is not None and dataclasses.is_dataclass(body):
            body = dataclasses.asdict(body)
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(self._base_url + path, data=data, method=method)
        if data is not None:
            req.add_header("Content-Type", "application/json")
        if self._api_key:
            req.add_header("X-API-Key", self._api_key)
        try:
            with self._opener.open(req) as resp:
                return resp.status, resp.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    @staticmethod
    def _raise(status: int, raw: bytes) -> None:
        try:
            decoded = json.loads(raw) if raw else {}
        except ValueError:
            decoded = {}
        if not isinstance(decoded, dict):
            decoded = {}
        raise ApiError(
            status,
            decoded.get("message", ""),
            decoded.get("slug", ""),
            decoded.get("hints"),
        )

    def list_books(self, genre, cursor=None, sort=None) -> ListBooksResponse:
        path = "/books/"
        _query = {}
        _query["genre"] = genre
        if cursor is not None:
            _query["cursor"] = cursor
        if sort is not None:
            _query["sort"] = sort
        if _query:
            path = path + "?" + urllib.parse.urlencode(_query)
        _status, _raw = self._do("GET", path)
        if _status != 200:
            self._raise(_status, _raw)
        _data = json.loads(_raw) if _raw else {}
        return ListBooksResponse.from_dict(_data)

    def create_book(self, body: Book) -> CreatedMessage:
        path = "/books/"
        _status, _raw = self._do("POST", path, body=body)
        if _status != 201:
            self._raise(_status, _raw)
        _data = json.loads(_raw) if _raw else {}
        return CreatedMessage.from_dict(_data)

    def get_book(self, book_id, fmt=None) -> BookOrError:
        path = f"/books/{urllib.parse.quote(str(book_id), safe='')}"
        _query = {}
        if fmt is not None:
            _query["fmt"] = fmt
        if _query:
            path = path + "?" + urllib.parse.urlencode(_query)
        _status, _raw = self._do("GET", path)
        if _status != 200:
            self._raise(_status, _raw)
        _data = json.loads(_raw) if _raw else {}
        return BookOrError.from_dict(_data)

    def update_book(self, book_id, body: BookFilters) -> CreatedMessage:
        path = f"/books/{urllib.parse.quote(str(book_id), safe='')}"
        _status, _raw = self._do("PUT", path, body=body)
        if _status != 200:
            self._raise(_status, _raw)
        _data = json.loads(_raw) if _raw else {}
        return CreatedMessage.from_dict(_data)
