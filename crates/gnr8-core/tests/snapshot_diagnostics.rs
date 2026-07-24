//! Contract test (FIX-03 / FIX-04): the expected diagnostics for the goalservice fixture — now GREEN.
//!
//! 02-03 implemented the `diagnostics::collect` seam, so the `.expect()` now succeeds and the test
//! asserts the real diagnostics text against the reviewed
//! `snapshots/snapshot_diagnostics__goalservice_diagnostics.snap`: 4 `WARN` lines reconciled with
//! `fixtures/goalservice/expected/diagnostics.txt` — free-form-map ×1,
//! untyped-query ×3 — normalized to one canonical template per rule and sorted by `(file, line)`.
//! Plain text → `assert_snapshot!`. The snapshot was authored from REAL output and reviewed. CI runs
//! insta in `INSTA_UPDATE=no` (`CI=true`), so a mismatch hard-fails — it never auto-accepts (FIX-04).
//!
//! Requires the Go toolchain (the test invokes the helper via `go run`).

// Tests legitimately use unwrap/expect (skill ch.4 + ch.5); scoped allow keeps RUST-04 intact
// for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture authored in Plan 01-02, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn diagnostics_match_expected_for_goalservice() {
    // 02-03: collect runs the goextract helper and renders the canonical 7-line WARN text; the
    // snapshot below locks it (reconciled with expected/diagnostics.txt).
    let diags = gnr8::diagnostics::collect(FIXTURE_DIR)
        .expect("diagnostics::collect must succeed (requires the Go toolchain)");
    insta::assert_snapshot!("goalservice_diagnostics", diags);
}
