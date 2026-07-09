from __future__ import annotations

from typing import Any, Optional


class ApiError(Exception):
    """Raised by operation methods on a non-success response.

    Carries status, response metadata, raw body, parsed JSON, and decoded error body.
    """

    def __init__(
        self,
        status_code: int,
        message: str = "",
        slug: str = "",
        hints: Optional[list[Any]] = None,
        *,
        headers: Optional[dict[str, str]] = None,
        request_id: str = "",
        raw_body: bytes = b"",
        json_body: Any = None,
        body: Any = None,
    ) -> None:
        super().__init__(f"{status_code} {message} ({slug})")
        self.status_code = status_code
        self.headers = headers or {}
        self.request_id = request_id
        self.raw_body = raw_body
        self.json_body = json_body
        self.body = body
        self.message = message
        self.slug = slug
        self.hints = hints if hints is not None else []

    def is_not_found(self) -> bool:
        return self.status_code == 404
