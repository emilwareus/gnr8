# OpenAPI Baseline

## Current Version Baseline

As of June 24, 2026, the official OpenAPI specification site lists OpenAPI 3.2.0 as the latest OpenAPI specification version.

Sources:

- <https://spec.openapis.org/oas/>
- <https://spec.openapis.org/oas/v3.2.0.html>

The OpenAPI Initiative announced OpenAPI v3.2.0 in September 2025.

Source: <https://www.openapis.org/blog/2025/09/23/announcing-openapi-v3-2>

## Why 3.2 Matters

OpenAPI 3.2 adds or clarifies areas that matter for modern APIs:

- Richer tag structure.
- Expanded HTTP method support, including QUERY and `additionalOperations`.
- Better support for streaming media types.
- Improvements that reduce reliance on vendor extensions for patterns that already exist in production APIs.

Source: <https://www.openapis.org/blog/2025/09/23/announcing-openapi-v3-2>

## Compatibility Concern

The product should probably model OpenAPI 3.2-level semantics internally, but output compatibility profiles:

- OpenAPI 3.2 for forward-looking users.
- OpenAPI 3.1 for JSON Schema alignment and broader near-term compatibility.
- OpenAPI 3.0.x if downstream generators or gateways require it.

Reasoning:

- Many Go tools and SDK generators still center on OpenAPI 3.0 or earlier.
- Some production pipelines prefer tool compatibility over latest-spec fidelity.
- A graph-first architecture can preserve richer meaning and lower to older profiles with explicit loss warnings.

## Internal Model Rule

Do not shape the core API graph like Swagger 2.0, OpenAPI 3.0, or OpenAPI 3.2 directly.

Instead:

- Model operations, parameters, request bodies, responses, schemas, auth, streaming, examples, and source provenance as internal graph facts.
- Lower graph facts into OpenAPI versions.
- Emit diagnostics when a target OpenAPI version cannot express a graph fact without loss.

## Open Questions

- Should the default output be OpenAPI 3.1 or 3.2 for the first public version?
- Which downstream validators should be part of CI?
- How should generated SDKs treat OpenAPI 3.2 streaming declarations?
- What is the compatibility contract for users who need OpenAPI 3.0 output?
- Should Arazzo or Overlays be in scope later, or treated as separate products?
