# gnr8 quality gates (D-16 + Go fixture gate).
#
# `make check` is the full LOCAL gate and mirrors CI. It runs fmt-check, clippy
# (--locked, -D warnings), the test suite, and the Go fixture build/vet.
#
# As of Phase 3 (D-07) the red-by-design era is OVER: all four contract tests
# (snapshot_graph/diagnostics/openapi/sdk) are GREEN, and `make gates` now runs them as the
# blocking set alongside determinism + the new sdk_compile test (temp dir + zero-require go.mod +
# go build + httptest smoke, SDK-05). The standalone `contract` target is retired — there are no
# longer any red-by-design failures to isolate. `make gates` mirrors the blocking CI `gates` job.

.PHONY: fmt fmt-check clippy test gates fixture-build goextract-build check all

# Auto-format the workspace in place.
fmt:
	cargo fmt --all

# Verify formatting without modifying files (CI-equivalent).
fmt-check:
	cargo fmt --all -- --check

# Lint with warnings denied; --locked requires a committed, up-to-date Cargo.lock (Pitfall 4).
clippy:
	cargo clippy --all-targets --all-features --locked -- -D warnings

# Full test suite — every test is now green (the red-by-design era ended in Phase 3).
test:
	cargo test --all-features

# Blocking gate test set: green unit + CLI parse tests (incl. the pure `watch::tests` loop-safety
# filter tests and the host→child→write `generate_e2e` integration test in `cargo test -p gnr8`), ALL
# FOUR contract tests (snapshot_graph/diagnostics/openapi/sdk), determinism (graph + OpenAPI + SDK
# byte-identical), sdk_compile (temp dir + zero-require go.mod + go build + httptest smoke, SDK-05), the
# `sdk_pipeline` SDK-framework integration test, and the `lifecycle` suite (manifest round-trip + the
# pure `plan_writes` truth table over synthetic Artifacts + the `.gnr8/` crate scaffold + the
# naming-override $ref rewrites). These invoke the goextract helper via `go run`, pipe Go through
# `gofmt`, run `go build`/`go test`, and (for `generate_e2e`) cargo-compile + run the scaffolded child
# crate, so the Go + cargo toolchains must be present. The timing-tolerant `watch_smoke` smoke is
# `#[ignore]`d (FS-event flakiness) and is therefore NOT in this blocking line — run it opt-in with
# `cargo test -p gnr8 --test watch_smoke -- --ignored`. Mirrors the CI `gates` job (RUST-03 / D-07).
gates:
	cargo test -p gnr8-core --lib
	cargo test -p gnr8
	cargo test -p gnr8-core --test snapshot_graph --test snapshot_diagnostics --test snapshot_openapi --test snapshot_sdk --test determinism --test sdk_compile --test sdk_pipeline --test lifecycle

# Compile + vet the standalone Go Gin fixture module (Pitfall 5 — cargo never builds it).
fixture-build:
	cd fixtures/goalservice && go build ./... && go vet ./...

# Build + vet + test the standalone goextract helper module (cargo never builds it).
# Mirrors the fixture-build gate; the helper is the Go side of the Rust<->Go contract.
goextract-build:
	cd goextract && go build ./... && go vet ./... && go test ./...

# Full local gate, mirrors CI. The whole suite is green now (no red-by-design failures remain).
check: fmt-check clippy test fixture-build goextract-build

all: check
