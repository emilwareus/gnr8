# gnr8 quality gates (D-16 + Go fixture gate).
#
# `make check` is the full LOCAL gate and mirrors CI. It runs fmt-check, clippy
# (--locked, -D warnings), the test suite, and the Go fixture build/vet.
#
# NOTE (red-by-design, FIX-04): `make test` runs the WHOLE suite, including the four
# red-by-design contract tests (snapshot_graph/openapi/sdk/diagnostics). Those FAIL on
# purpose today — they call gnr8-core seams that still return NotYetImplemented, so the
# .expect() panics. That redness is the contract a developer sees locally. Use
# `make gates` to run only the genuinely-green blocking gate the CI `gates` job enforces
# (lib + bin tests, excluding the integration `tests/` dir), and `make contract` to run
# the four red tests on their own.

.PHONY: fmt fmt-check clippy test gates contract fixture-build goextract-build check all

# Auto-format the workspace in place.
fmt:
	cargo fmt --all

# Verify formatting without modifying files (CI-equivalent).
fmt-check:
	cargo fmt --all -- --check

# Lint with warnings denied; --locked requires a committed, up-to-date Cargo.lock (Pitfall 4).
clippy:
	cargo clippy --all-targets --all-features --locked -- -D warnings

# Full test suite — INCLUDES the four red-by-design contract tests (they fail on purpose, FIX-04).
test:
	cargo test --all-features

# Blocking gate test set: genuinely-green unit + CLI parse tests only.
# Excludes the integration `tests/` dir so the four red-by-design tests do NOT fail this target.
# This mirrors the CI `gates` job (RUST-03 / Open Q1 option d).
gates:
	cargo test -p gnr8-core --lib
	cargo test -p gnr8

# The four red-by-design contract tests, run on their own. VISIBLY RED until Phases 2-3 (FIX-04).
# Mirrors the non-blocking CI `contract` job; promoted to blocking in Phase 3.
contract:
	cargo test -p gnr8-core --test snapshot_graph --test snapshot_openapi --test snapshot_sdk --test snapshot_diagnostics

# Compile + vet the standalone Go Gin fixture module (Pitfall 5 — cargo never builds it).
fixture-build:
	cd fixtures/goalservice && go build ./... && go vet ./...

# Build + vet + test the standalone goextract helper module (cargo never builds it).
# Mirrors the fixture-build gate; the helper is the Go side of the Rust<->Go contract.
goextract-build:
	cd goextract && go build ./... && go vet ./... && go test ./...

# Full local gate, mirrors CI. `test` surfaces the red-by-design contract failures by design.
check: fmt-check clippy test fixture-build goextract-build

all: check
