//! Red-by-design contract test (FIX-03 / FIX-04): the expected diagnostics for the goalservice fixture.
//!
//! Calls the Phase-2+ `diagnostics::collect` seam, which today returns
//! `CoreError::NotYetImplemented`, so the `.expect()` panics BEFORE the snapshot assertion runs →
//! the test FAILS CLEARLY (red-by-design, per RESEARCH Pattern 4 mechanism 1). It is never marked
//! ignored (FIX-04), and there is no pre-authored `.snap`. Diagnostics are plain text (the
//! float64->float32 narrowing, the untyped query param, the free-form `map[string]any` warnings),
//! so the snapshot uses `assert_snapshot!`. Phase 2+ implements `collect` → the snapshot is
//! reviewed/accepted → the test turns green.

// Tests legitimately use unwrap/expect (skill ch.4 + ch.5); scoped allow keeps RUST-04 intact
// for production code (Pitfall 2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

/// The Go Gin fixture authored in Plan 01-02, resolved relative to this crate's manifest dir.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

#[test]
fn diagnostics_match_expected_for_goalservice() {
    let diags = gnr8_core::diagnostics::collect(FIXTURE_DIR)
        .expect("Phase 2+ must implement diagnostics::collect; red-by-design until then");
    insta::assert_snapshot!("goalservice_diagnostics", diags);
}
