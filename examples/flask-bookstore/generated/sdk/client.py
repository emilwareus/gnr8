from __future__ import annotations

import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Optional

from pydantic import BaseModel

from .errors import ApiError
from .models import *  # noqa: F401,F403  (re-export models for return-type annotations)


class Client:
    """SDK client over urllib (no requests/httpx)."""

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
        # Pydantic v2 request models need alias-aware JSON-mode dumping before json.dumps.
        if isinstance(body, BaseModel):
            body = body.model_dump(mode="json", by_alias=True, exclude_unset=True)

        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(self._base_url + path, data=data, method=method)
        if data is not None:
            req.add_header("Content-Type", "application/json")
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

    def list_orders(self, status=None) -> OrderConfirmation:
        path = "/orders/"
        _query = {}
        if status is not None:
            _query["status"] = status
        if _query:
            path = path + "?" + urllib.parse.urlencode(_query)
        _status, _raw = self._do("GET", path)
        if _status != 200:
            self._raise(_status, _raw)
        _data = json.loads(_raw) if _raw else {}
        return OrderConfirmation.model_validate(_data)

    def create_order(self, body: OrderInput) -> OrderConfirmation:
        path = "/orders/"
        _status, _raw = self._do("POST", path, body=body)
        if _status != 201:
            self._raise(_status, _raw)
        _data = json.loads(_raw) if _raw else {}
        return OrderConfirmation.model_validate(_data)

    def create_order_raw(self) -> Any:
        path = "/orders/raw"
        _status, _raw = self._do("POST", path)
        if _status != 200:
            self._raise(_status, _raw)
        return json.loads(_raw) if _raw else None

    def get_order(self, order_id) -> OrderConfirmation:
        path = f"/orders/{urllib.parse.quote(str(order_id), safe='')}"
        _status, _raw = self._do("GET", path)
        if _status != 200:
            self._raise(_status, _raw)
        _data = json.loads(_raw) if _raw else {}
        return OrderConfirmation.model_validate(_data)
