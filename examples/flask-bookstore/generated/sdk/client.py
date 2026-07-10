from __future__ import annotations

import enum
import json
import secrets
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from typing import Any, Optional

from pydantic import BaseModel

from .errors import ApiError
from .models import (
    OrderConfirmation,
    OrderInput,
)


class RequestOptions:
    """Per-request SDK runtime overrides."""

    def __init__(
        self,
        *,
        timeout: Optional[float] = None,
        max_retries: Optional[int] = None,
        idempotency_key: Optional[str] = None,
        metadata: Optional[dict[str, str]] = None,
    ) -> None:
        self.timeout = timeout
        self.max_retries = max_retries
        self.idempotency_key = idempotency_key
        self.metadata = metadata or {}


class HookContext:
    """Context passed to generated SDK runtime hooks."""

    def __init__(
        self,
        *,
        operation_id: str,
        method: str,
        path_template: str,
        url: str,
        headers: dict[str, str],
        request_metadata: dict[str, str],
    ) -> None:
        self.operation_id = operation_id
        self.method = method
        self.path_template = path_template
        self.url = url
        self.headers = headers
        self.request_metadata = request_metadata
        self.status: Optional[int] = None
        self.response_headers: dict[str, str] = {}


class ClientHooks:
    """Generated SDK runtime hooks."""

    def __init__(
        self,
        *,
        request: Optional[
            list[Callable[[HookContext, urllib.request.Request], None]]
        ] = None,
        response: Optional[list[Callable[[HookContext], None]]] = None,
        error: Optional[list[Callable[[HookContext, BaseException], None]]] = None,
    ) -> None:
        self.request = request or []
        self.response = response or []
        self.error = error or []


class Client:
    """SDK client over urllib (no requests/httpx)."""

    def __init__(
        self,
        base_url: str,
        *,
        api_key: Optional[str] = None,
        opener: Optional[urllib.request.OpenerDirector] = None,
        timeout: Optional[float] = 30.0,
        max_retries: int = 0,
        hooks: Optional[ClientHooks] = None,
    ) -> None:
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._opener = opener or urllib.request.build_opener()
        self._timeout = timeout
        self._max_retries = max_retries
        self._retry_statuses = (408, 429)
        self._retry_unsafe_methods = False
        self._hooks = hooks or ClientHooks()

    def _body_value(self, body: Any, body_encoding: str) -> Any:
        if isinstance(body, BaseModel):
            mode = "python" if body_encoding == "multipart" else "json"
            body = body.model_dump(mode=mode, by_alias=True, exclude_unset=True)
        return self._wire_value(body)

    def _wire_value(self, value: Any) -> Any:
        if isinstance(value, enum.Enum):
            return self._wire_value(value.value)
        if isinstance(value, list):
            return [self._wire_value(item) for item in value]
        if isinstance(value, tuple):
            return tuple(self._wire_value(item) for item in value)
        if isinstance(value, dict):
            return {key: self._wire_value(item) for key, item in value.items()}
        return value

    def _encode_body(
        self,
        body: Optional[Any],
        body_encoding: str,
        content_type: str,
    ) -> tuple[Optional[bytes], str]:
        if body is None:
            return None, content_type
        if body_encoding == "binary":
            if isinstance(body, bytes):
                return body, content_type
            if isinstance(body, bytearray):
                return bytes(body), content_type
            raise TypeError("binary request bodies must be bytes or bytearray")
        if body_encoding == "text":
            return str(body).encode(), content_type
        value = self._body_value(body, body_encoding)
        if body_encoding == "json":
            return json.dumps(value).encode(), content_type
        if body_encoding == "form":
            encoded = urllib.parse.urlencode(value, doseq=True).encode()
            return encoded, content_type
        if body_encoding == "multipart":
            boundary = f"gnr8-{secrets.token_hex(16)}"
            return (
                self._encode_multipart(value, boundary),
                f"multipart/form-data; boundary={boundary}",
            )
        raise ValueError(f"unsupported request body encoding: {body_encoding}")

    def _encode_multipart(self, value: Any, boundary: str) -> bytes:
        if not isinstance(value, dict):
            raise TypeError("multipart request bodies must encode to a dict")
        out = bytearray()
        for key, item in value.items():
            if item is None:
                continue
            items = item if isinstance(item, (list, tuple)) else (item,)
            for part in items:
                if part is None:
                    continue
                out.extend(f"--{boundary}\r\n".encode())
                if isinstance(part, (bytes, bytearray)):
                    out.extend(
                        (
                            f'Content-Disposition: form-data; name="{key}"; '
                            f'filename="{key}"\r\n'
                            "Content-Type: application/octet-stream\r\n\r\n"
                        ).encode()
                    )
                    out.extend(bytes(part))
                    out.extend(b"\r\n")
                else:
                    out.extend(
                        f'Content-Disposition: form-data; name="{key}"\r\n\r\n'.encode()
                    )
                    out.extend(str(part).encode())
                    out.extend(b"\r\n")
        out.extend(f"--{boundary}--\r\n".encode())
        return bytes(out)

    def _do(
        self,
        method: str,
        path: str,
        *,
        body: Optional[Any] = None,
        operation_id: str,
        path_template: str,
        content_type: str = "application/json",
        body_encoding: str = "json",
        request_options: Optional[RequestOptions] = None,
        idempotent: bool = False,
        idempotency_key_header: str = "Idempotency-Key",
    ) -> tuple:
        data, content_type = self._encode_body(body, body_encoding, content_type)
        options = request_options or RequestOptions()
        timeout = options.timeout if options.timeout is not None else self._timeout
        if options.max_retries is not None:
            max_retries = options.max_retries
        else:
            max_retries = self._max_retries
        if max_retries < 0:
            max_retries = 0
        if not (
            self._retry_unsafe_methods
            or idempotent
            or method in ("GET", "HEAD", "OPTIONS", "PUT", "DELETE")
        ):
            max_retries = 0
        headers: dict[str, str] = {}
        if data is not None:
            headers["Content-Type"] = content_type
        if idempotent and options.idempotency_key:
            headers[idempotency_key_header] = options.idempotency_key
        url = self._base_url + path
        last_error: Optional[BaseException] = None
        for attempt in range(max_retries + 1):
            req = urllib.request.Request(url, data=data, method=method)
            for key, value in headers.items():
                req.add_header(key, value)
            context = HookContext(
                operation_id=operation_id,
                method=method,
                path_template=path_template,
                url=url,
                headers=dict(headers),
                request_metadata=dict(options.metadata),
            )
            try:
                for hook in self._hooks.request:
                    hook(context, req)
                try:
                    with self._opener.open(req, timeout=timeout) as resp:
                        status = resp.status
                        response_headers = dict(resp.headers.items())
                        raw = resp.read()
                except urllib.error.HTTPError as e:
                    status = e.code
                    response_headers = dict(e.headers.items())
                    raw = e.read()
                context.status = status
                context.response_headers = response_headers
                for hook in self._hooks.response:
                    hook(context)
                if self._should_retry_status(status) and attempt < max_retries:
                    self._sleep_retry_after(response_headers)
                    continue
                if status < 200 or status >= 300:
                    self._call_error_hooks(
                        context,
                        ApiError(
                            status,
                            "",
                            "",
                            headers=response_headers,
                            raw_body=raw,
                        ),
                    )
                return status, response_headers, raw
            except urllib.error.URLError as e:
                last_error = e
                if attempt < max_retries:
                    continue
                self._call_error_hooks(context, e)
                raise
        if last_error is not None:
            raise last_error
        raise RuntimeError("request failed without response")

    def _should_retry_status(self, status: int) -> bool:
        return status in self._retry_statuses or status >= 500

    @staticmethod
    def _sleep_retry_after(headers: dict[str, str]) -> None:
        retry_after = headers.get("Retry-After") or headers.get("retry-after")
        if not retry_after:
            return
        try:
            seconds = int(retry_after)
        except ValueError:
            return
        if seconds > 0:
            time.sleep(seconds)

    def _call_error_hooks(self, context: HookContext, error: BaseException) -> None:
        for hook in self._hooks.error:
            hook(context, error)

    @staticmethod
    def _raise(
        status: int,
        headers: dict[str, str],
        raw: bytes,
        error_model: Optional[type] = None,
    ) -> None:
        try:
            json_body = json.loads(raw) if raw else None
        except ValueError:
            json_body = None
        body = json_body
        if error_model is not None and isinstance(json_body, dict):
            try:
                body = error_model.from_dict(json_body)
            except Exception:
                body = json_body
        decoded = json_body if isinstance(json_body, dict) else {}
        request_id = headers.get("X-Request-ID") or headers.get("x-request-id", "")
        raise ApiError(
            status,
            decoded.get("message", ""),
            decoded.get("slug", ""),
            decoded.get("hints"),
            headers=headers,
            request_id=request_id,
            raw_body=raw,
            json_body=json_body,
            body=body,
        )

    def list_orders(
        self,
        status=None,
        request_options: Optional[RequestOptions] = None,
    ) -> OrderConfirmation:
        path = "/orders/"
        _query = {}
        if status is not None:
            _query["status"] = status
        if _query:
            path = path + "?" + urllib.parse.urlencode(_query)
        _status, _headers, _raw = self._do(
            "GET",
            path,
            operation_id="list_orders",
            path_template="/",
            request_options=request_options,
            idempotent=False,
            idempotency_key_header="Idempotency-Key",
        )
        if _status < 200 or _status >= 300:
            self._raise(_status, _headers, _raw)
        if _status in (200,):
            _data = json.loads(_raw) if _raw else {}
            return OrderConfirmation.model_validate(_data)
        self._raise(_status, _headers, _raw)

    def create_order(
        self,
        body: OrderInput,
        request_options: Optional[RequestOptions] = None,
    ) -> OrderConfirmation:
        path = "/orders/"
        _status, _headers, _raw = self._do(
            "POST",
            path,
            body=body,
            content_type="application/json",
            body_encoding="json",
            operation_id="create_order",
            path_template="/",
            request_options=request_options,
            idempotent=False,
            idempotency_key_header="Idempotency-Key",
        )
        if _status < 200 or _status >= 300:
            self._raise(_status, _headers, _raw)
        if _status in (201,):
            _data = json.loads(_raw) if _raw else {}
            return OrderConfirmation.model_validate(_data)
        self._raise(_status, _headers, _raw)

    def create_order_raw(self, request_options: Optional[RequestOptions] = None) -> Any:
        path = "/orders/raw"
        _status, _headers, _raw = self._do(
            "POST",
            path,
            operation_id="create_order_raw",
            path_template="/raw",
            request_options=request_options,
            idempotent=False,
            idempotency_key_header="Idempotency-Key",
        )
        if _status < 200 or _status >= 300:
            self._raise(_status, _headers, _raw)
        return json.loads(_raw) if _raw else None

    def get_order(
        self,
        order_id,
        request_options: Optional[RequestOptions] = None,
    ) -> OrderConfirmation:
        path = f"/orders/{urllib.parse.quote(str(order_id), safe='')}"
        _status, _headers, _raw = self._do(
            "GET",
            path,
            operation_id="get_order",
            path_template="/{order_id}",
            request_options=request_options,
            idempotent=False,
            idempotency_key_header="Idempotency-Key",
        )
        if _status < 200 or _status >= 300:
            self._raise(_status, _headers, _raw)
        if _status in (200,):
            _data = json.loads(_raw) if _raw else {}
            return OrderConfirmation.model_validate(_data)
        self._raise(_status, _headers, _raw)
