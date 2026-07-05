from __future__ import annotations

from typing import Any, Optional


class ApiError(Exception):
    """Raised by operation methods on a non-success response.

    Carries the HTTP status and the decoded error body (message/slug/hints).
    """

    def __init__(
        self,
        status_code: int,
        message: str = "",
        slug: str = "",
        hints: Optional[list[Any]] = None,
    ) -> None:
        super().__init__(f"{status_code} {message} ({slug})")
        self.status_code = status_code
        self.message = message
        self.slug = slug
        self.hints = hints if hints is not None else []

    def is_not_found(self) -> bool:
        return self.status_code == 404
