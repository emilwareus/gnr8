# Deferred Items — Phase 04

Out-of-scope discoveries logged during execution (NOT fixed; outside the current task's changes).

## 04-01

- **Pre-existing `cargo fmt` drift in `crates/gnr8-core/src/sdk/emit.rs`** (Phase-3 file).
  `cargo fmt --all` reformats two long lines (an `if` chain at ~L499 and a test `assert!` at
  ~L1483) that were already unformatted at the start of plan 04-01. Reverted to keep the 04-01
  commit scoped to lifecycle/config changes (scope boundary). All files 04-01 touched are
  fmt-clean. Fix belongs in a Phase-3 touch-up or a dedicated `style:` chore commit, not 04-01.
