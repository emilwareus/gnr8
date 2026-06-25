//! `gnr8 watch` timing-tolerant smoke test (WATCH-02): one source edit → exactly one regeneration
//! signal; gnr8's own output write does NOT trigger a second one (no self-loop).
//!
//! This drives the REAL `notify-debouncer-full` debouncer + the binary's pure loop-safety filter
//! (`is_trigger_path` / `batch_should_regenerate`, mirrored here since they are `pub(crate)` in the
//! binary) over a hermetic temp dir — the "smoke the shell" half of RESEARCH Pitfall 3's "test the
//! decision, smoke the shell". The pure decision itself is exhaustively unit-tested in `watch.rs`
//! (`output_paths_filtered` / `go_source_triggers` / `non_go_ignored` / `source_wins_over_output`),
//! which run in the blocking gates; THIS test depends on real filesystem-event timing and is therefore
//! `#[ignore]`d out of the blocking set (run it explicitly with `cargo test -p gnr8 --test watch_smoke
//! -- --ignored`). Mirrors the Phase-3 graceful-skip precedent for environment-dependent tests.

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4); scope the allow to this
// test target so the workspace-wide RUST-04 deny stays intact for production code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};

/// Create a UNIQUE temp subdir under `std::env::temp_dir()` (PID + nanos — no `tempfile` crate, the
/// Phase-3 hermetic-temp precedent, T-03-03-SC).
fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir =
        std::env::temp_dir().join(format!("gnr8-watch-{label}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

/// The loop-safety filter, mirrored from the binary's `watch.rs` (the functions are `pub(crate)` there
/// and so cannot be imported by an integration test). The smoke test asserts the SAME decision the
/// shell uses: a `*.go` source path NOT under an output path triggers; an output-path write does not.
fn is_trigger_path(path: &Path, output_set: &HashSet<PathBuf>) -> bool {
    if output_set
        .iter()
        .any(|out| path == out || path.starts_with(out))
    {
        return false;
    }
    path.extension().is_some_and(|ext| ext == "go")
}

fn batch_should_regenerate(paths: &[PathBuf], output_set: &HashSet<PathBuf>) -> bool {
    paths.iter().any(|p| is_trigger_path(p, output_set))
}

/// Canonicalize a path, falling back to the raw path on failure — mirrors the shell's loop-safety fix
/// (macOS reports `/private/var/...` for a `/var/...` temp dir, so both the output set and the event
/// paths must be canonicalized before comparison or gnr8's own writes would slip through the filter).
fn canon(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// One source `.go` edit produces exactly one regeneration signal; a subsequent write to an OUTPUT path
/// (simulating gnr8's own write) produces NONE — proving the watch loop cannot self-trigger (WATCH-02).
///
/// `#[ignore]`d: it depends on real FS-event delivery timing (non-deterministic across platforms/CI),
/// so it is opt-in, not part of the blocking gate (the pure filter unit tests cover loop-safety
/// deterministically). Run with `cargo test -p gnr8 --test watch_smoke -- --ignored`.
#[test]
#[ignore = "timing-dependent FS-event smoke; run opt-in with `-- --ignored` (loop-safety is covered \
            deterministically by the pure watch::tests unit tests in the blocking gates)"]
fn single_edit_one_regen() {
    let root = unique_temp_dir("smoke");
    let src_dir = root.join("src");
    let out_dir = root.join("sdk");
    std::fs::create_dir_all(&src_dir).expect("create src dir");
    std::fs::create_dir_all(&out_dir).expect("create sdk dir");

    // The output set the filter drops (the OpenAPI artifact + the whole SDK dir) — gnr8's own writes.
    // Canonicalized so it matches the canonical paths notify reports (the loop-safety fix).
    let mut output_set: HashSet<PathBuf> = HashSet::new();
    output_set.insert(canon(&root.join("openapi.yaml")));
    output_set.insert(canon(&out_dir));

    // Count REAL regeneration signals: the debouncer callback applies the same pure filter the shell
    // uses and sends `()` for each qualifying batch.
    let (tx, rx) = mpsc::channel::<()>();
    let filter_set = output_set.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        None,
        move |result: DebounceEventResult| {
            if let Ok(events) = result {
                let paths: Vec<PathBuf> = events
                    .iter()
                    .flat_map(|ev| ev.paths.iter())
                    .map(|p| canon(p))
                    .collect();
                if batch_should_regenerate(&paths, &filter_set) {
                    let _ = tx.send(());
                }
            }
            // A notify error must NOT crash the smoke loop (mirrors the shell): ignore it here.
        },
    )
    .expect("create debouncer");

    // Watch BOTH the source dir AND the output dir — proving that even when the output dir is watched,
    // gnr8's own writes under it are filtered out (the belt-and-braces layer). The shell watches only
    // source dirs (the primary defense); watching the output dir here is a strictly harder test.
    debouncer
        .watch(&src_dir, RecursiveMode::Recursive)
        .expect("watch src dir");
    debouncer
        .watch(&out_dir, RecursiveMode::Recursive)
        .expect("watch out dir");

    // Let the watcher settle before the first edit (FSEvents warm-up).
    std::thread::sleep(Duration::from_millis(300));

    // (1) Edit ONE source `.go` file → expect exactly one regeneration signal.
    std::fs::write(
        src_dir.join("handlers.go"),
        b"package main\n\nfunc Handler() {}\n",
    )
    .expect("write source file");

    rx.recv_timeout(Duration::from_secs(5))
        .expect("a source `.go` edit must produce one regeneration signal within 5s");

    // (2) Simulate gnr8's OWN output writes (OpenAPI + an SDK file under the watched output dir).
    // These are output paths → the filter drops them → NO regeneration signal (no self-loop).
    std::fs::write(root.join("openapi.yaml"), b"openapi: 3.1.0\n").expect("write openapi output");
    std::fs::write(out_dir.join("client.go"), b"package sdk\n").expect("write sdk output");

    // Assert NO further signal arrives in a generous window: the output writes did not loop.
    match rx.recv_timeout(Duration::from_secs(2)) {
        Err(mpsc::RecvTimeoutError::Timeout) => { /* expected — output writes are filtered out */
        }
        Ok(()) => panic!("gnr8's own output write must NOT trigger a regeneration (self-loop!)"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("debouncer channel disconnected unexpectedly")
        }
    }

    drop(debouncer);
    let _ = std::fs::remove_dir_all(&root);
}
