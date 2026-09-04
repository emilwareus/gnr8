from __future__ import annotations

import email.policy
import email.parser
import json
import urllib.parse
from typing import Any

from example_wire import (
    Client,
    ClientHooks,
    RequestOptions,
    UpdateItemRequest,
    UploadFileFormRequest,
)


class FakeResponse:
    def __init__(self, status: int, headers: dict[str, str] | None = None, body: bytes = b"") -> None:
        self.status = status
        self.headers = headers or {}
        self.body = body

    def __enter__(self) -> FakeResponse:
        return self

    def __exit__(self, *_args: Any) -> None:
        return None

    def read(self) -> bytes:
        return self.body


class FakeOpener:
    def __init__(self, redirect: bool = False) -> None:
        self.redirect = redirect
        self.requests: list[Any] = []

    def open(self, request: Any, timeout: float | None = None) -> FakeResponse:
        del timeout
        self.requests.append(request)
        if request.full_url.endswith("/redirect"):
            return FakeResponse(
                307,
                {"Location": "/v1/files/final", "X-Session-ID": "session-123"},
            )
        if urllib.parse.urlsplit(request.full_url).path.endswith("/search"):
            return FakeResponse(
                200,
                {"Content-Type": "application/json"},
                json.dumps(
                    {
                        "q": "",
                        "limit": 0,
                        "offset": 0,
                        "page": 1,
                        "days": 0,
                        "sort": "",
                        "cursor": "",
                        "token": "",
                    }
                ).encode(),
            )
        return FakeResponse(204)


def multipart_parts(request: Any) -> dict[str, list[bytes]]:
    content_type = request.get_header("Content-type")
    assert content_type and content_type.startswith("multipart/form-data; boundary=")
    message = email.parser.BytesParser(policy=email.policy.default).parsebytes(
        b"Content-Type: " + content_type.encode() + b"\r\nMIME-Version: 1.0\r\n\r\n" + request.data
    )
    parts: dict[str, list[bytes]] = {}
    for part in message.iter_parts():
        name = part.get_param("name", header="content-disposition")
        assert isinstance(name, str)
        parts.setdefault(name, []).append(part.get_payload(decode=True))
    return parts


def main() -> None:
    default_opener = FakeOpener()
    redirect_opener = FakeOpener(redirect=True)
    response_contexts: list[Any] = []
    client = Client(
        "https://api.test",
        opener=default_opener,
        hooks=ClientHooks(response=[response_contexts.append]),
    )
    client._redirect_opener = redirect_opener

    client.upload_file(("application/json", UpdateItemRequest(name="json")))
    json_request = default_opener.requests[-1]
    assert json_request.get_header("Content-type") == "application/json"
    assert json.loads(json_request.data) == {"name": "json"}

    cases = [
        UploadFileFormRequest(),
        UploadFileFormRequest(request='{"name":"multipart"}', files=[b"one"]),
        UploadFileFormRequest(request='{"name":"multipart"}', files=[b"one", b"two"]),
    ]
    expected_files = [[], [b"one"], [b"one", b"two"]]
    for body, expected in zip(cases, expected_files, strict=True):
        client.upload_file(("multipart/form-data", body))
        parts = multipart_parts(default_opener.requests[-1])
        assert parts.get("files", []) == expected, parts
        if expected:
            assert parts["request"] == [b'{"name":"multipart"}'], parts
        else:
            assert "request" not in parts and "files" not in parts, parts

    client.search_items(1, "")
    client.search_items(1, "", offset=0)
    search_urls = [
        request.full_url
        for request in default_opener.requests
        if urllib.parse.urlsplit(request.full_url).path.endswith("/search")
    ]
    first_query = urllib.parse.parse_qs(urllib.parse.urlsplit(search_urls[0]).query, keep_blank_values=True)
    second_query = urllib.parse.parse_qs(urllib.parse.urlsplit(search_urls[1]).query, keep_blank_values=True)
    assert "offset" not in first_query and "sort" not in first_query and "cursor" not in first_query
    assert second_query["offset"] == ["0"]

    client.redirect_file("file-1")
    client.redirect_file("file-1", RequestOptions(follow_redirects=True))
    assert len([request for request in default_opener.requests if request.full_url.endswith("/redirect")]) == 1
    assert len(redirect_opener.requests) == 1
    redirect_contexts = [context for context in response_contexts if context.operation_id == "redirectFile"]
    assert [context.status for context in redirect_contexts] == [307, 307]
    assert all(context.response_headers["X-Session-ID"] == "session-123" for context in redirect_contexts)


if __name__ == "__main__":
    main()
