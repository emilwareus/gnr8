# Research Index

This folder holds source-backed research for `gnr8`.

The current research question:

> Can a Rust tool own a fast, extensible, code-first pipeline from Go application code to OpenAPI and SDKs, with save-time incremental generation and code-based customization?

## Documents

- [Go and OpenAPI tooling](go-openapi-tooling.md)
- [Native Go code to OpenAPI](native-go-to-openapi.md)
- [SDK generation and structure](sdk-generation-and-structure.md)
- [Code-as-config and CLI UX](code-as-config-and-ux.md)
- [Generation lifecycle](generation-lifecycle.md)
- [Multi-language sources and targets](multi-language-sources-and-targets.md)
- [OpenAPI baseline](openapi-baseline.md)
- [Go static analysis](go-static-analysis.md)
- [Speed and incrementality](speed-and-incrementality.md)
- [Validation plan](validation-plan.md)

## Current Position

The opportunity appears credible, but the hardest parts are not template generation. The hard parts are:

- Accurate extraction of route and type semantics from real Go applications.
- Incremental invalidation that is finer than "rerun the generator".
- Compatibility with OpenAPI versions that downstream tooling actually accepts.
- A plugin model that is powerful without turning every framework adapter into bespoke code.
- A user experience where code is configuration, likely under `.gnr8/`, rather than a YAML-driven generator.
- Guardrails against overengineering: one vertical slice first, generalize only after repeated pressure.

## Global Logs

- Target architecture is tracked in [`../ARCHITECTURE.md`](../ARCHITECTURE.md).
- Rough PoC roadmap is tracked in [`../ROADMAP.md`](../ROADMAP.md).
- Feature candidates and scope decisions are tracked in [`../FEATURE.md`](../FEATURE.md).
- Product and architecture decisions are tracked in [`../DECISION.md`](../DECISION.md).

## Source Quality Rules

Prefer primary sources:

- Official spec pages.
- Official project repositories.
- Official Go documentation.
- Maintainer-authored docs.

Use issues and discussions only to establish real user pain or roadmap uncertainty, not as authoritative documentation.
