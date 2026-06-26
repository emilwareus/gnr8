"""Deterministic facts marshal — the contract boundary (twin of ``facts.go``).

The emitted JSON must deserialize into the Rust ``GoFacts`` DTO under
``deny_unknown_fields`` — any extra/missing key fails the host. Determinism
(identical input -> byte-identical output) is achieved two ways, both required:

  * ``json.dumps(..., sort_keys=True, ...)`` orders every object's KEYS.
  * :func:`_sort_doc` orders every ARRAY by the exact keys ``facts.go::sortDoc``
    uses — schemas by id; object fields by ``json_name``; enum members lexically;
    diagnostics by ``(file, line, message)``; routes by ``(path, method)``; params by
    ``(name, location)``; responses by status. Union members are NOT sorted (source
    order — RESEARCH Pitfall 5).
"""

import json


def build_doc(module, routes, schemas, diagnostics):
    """Assemble the top-level neutral facts dict.

    ``routes`` is empty in Plan 02-02 (FastAPI recognizer lands in Plan 03, Flask in
    Plan 04); the key is still present and emitted as ``[]``.
    """
    return {
        "module": module,
        "routes": list(routes),
        "schemas": list(schemas),
        "diagnostics": list(diagnostics),
    }


def marshal(doc):
    """Return the sorted, byte-stable JSON string for a facts ``doc``.

    Sorts every array in place (``_sort_doc``) then dumps with sorted object keys and
    compact separators. ``ensure_ascii=False`` keeps diagnostic text (e.g. ``->``)
    readable and stable, mirroring goextract's ``SetEscapeHTML(false)``.
    """
    _sort_doc(doc)
    return json.dumps(
        doc, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    )


def _sort_doc(doc):
    doc["schemas"].sort(key=lambda s: s["id"])
    for schema in doc["schemas"]:
        _sort_type(schema["body"])

    doc["diagnostics"].sort(
        key=lambda d: (d["file"], d["line"], d["message"])
    )

    doc["routes"].sort(key=lambda r: (r["path"], r["method"]))
    for route in doc["routes"]:
        _sort_route(route)


def _sort_type(t):
    """Recursively order the deterministic parts of a Type body.

    Object fields by ``json_name``; enum members lexically; recurse through array /
    map / union / object payloads. Union member ORDER is preserved (source order),
    but each member's inner type is still recursively sorted.
    """
    if not isinstance(t, dict):
        return
    kind = t.get("type")
    payload = t.get("of")

    if kind == "object" and isinstance(payload, list):
        payload.sort(key=lambda f: f["json_name"])
        for field in payload:
            _sort_type(field["schema"])
    elif kind == "enum" and isinstance(payload, list):
        payload.sort()
    elif kind == "union" and isinstance(payload, list):
        # Source order preserved — only recurse into members.
        for member in payload:
            _sort_type(member)
    elif kind == "array":
        _sort_type(payload)
    elif kind == "map" and isinstance(payload, dict):
        _sort_type(payload.get("key"))
        _sort_type(payload.get("value"))


def _sort_route(route):
    route["params"].sort(key=lambda p: (p["name"], p["location"]))
    for param in route["params"]:
        _sort_type(param["schema"])
    route["responses"].sort(key=lambda r: r["status"])
