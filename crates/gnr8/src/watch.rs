//! `gnr8 watch` — the loop-safe, debounced file watcher (WATCH-02 / WATCH-03, D-06/D-07).
//!
//! The watcher itself (`notify-debouncer-full`) lives at the binary boundary alongside `anyhow`
//! (D-09); `gnr8-core` stays watcher-free and unit-testable. This module follows the RESEARCH
//! "test the decision, smoke the shell" split (Pattern 4 / Pitfall 3):
//!
//! - The PURE decision — [`is_trigger_path`] / [`batch_should_regenerate`] — answers "should this
//!   changed path trigger a regeneration?" with NO watcher and NO I/O, so the loop-safety guarantee
//!   (WATCH-02) is proven by fast, non-flaky unit tests rather than timing-dependent integration runs.
//! - The thin I/O SHELL — [`run`] — owns the `notify-debouncer-full` debouncer, the source-dir-only
//!   watch (the primary loop defense), the `Instant` latency timing, and the std `AtomicBool` Ctrl-C
//!   shutdown. It never panics: a `notify` error is logged and the loop continues (RESEARCH Pitfall 1,
//!   threat T-04-03-03).
//!
//! ## Loop safety (WATCH-02, threat T-04-03-01)
//!
//! `notify` has NO built-in "ignore my own writes". gnr8's two-layer defense:
//!
//! 1. **watch SOURCE dirs only**, never the configured output dirs (primary — `run` only calls
//!    `debouncer.watch` on `config.inputs`);
//! 2. **drop output-path events** in the pure filter (belt-and-braces — a user may configure outputs
//!    *inside* a watched source tree, RESEARCH Pitfall 6).
//!
//! A debounced batch triggers a regeneration only if it contains at least one `*.go` source path that
//! is NOT under any output path. gnr8's own writes (the OpenAPI file + every SDK file) are output paths,
//! so they are filtered out and never re-trigger — the watch loop cannot loop.

// These module/item docs are dense with proper nouns/acronyms (OpenAPI, FSEvents, SDK, Ctrl-C, ...);
// backticking them would hurt readability. Allow `doc_markdown` module-wide (skill ch.2.4; mirrors the
// scoped allow in gnr8/src/cli.rs + gnr8-core/src/config/mod.rs).
#![allow(clippy::doc_markdown)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use gnr8_core::config::Config;
use gnr8_core::lifecycle::{self, GenerateOutcome};
use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};

/// One regeneration's latency + counts — the `--json` shape for WATCH-03.
///
/// `scenario` is one of `"cold"` / `"warm-noop"` / `"single-file-edit"`; `millis` is the wall-clock
/// duration of the `regenerate` call; `written`/`unchanged` are the per-bucket counts. The same struct
/// renders both the human line and (under `--json`) a machine-readable record.
#[derive(Debug, serde::Serialize)]
struct LatencyReport {
    /// Which WATCH-03 scenario this measurement is: `cold` | `warm-noop` | `single-file-edit`.
    scenario: String,
    /// Wall-clock milliseconds the `regenerate` call took (`Instant::elapsed`).
    millis: u128,
    /// Number of files written this regeneration.
    written: usize,
    /// Number of files byte-identical and therefore not rewritten (no-op).
    unchanged: usize,
}

impl LatencyReport {
    /// Build a report from a timed [`GenerateOutcome`].
    fn from_outcome(scenario: &str, elapsed: Duration, outcome: &GenerateOutcome) -> Self {
        Self {
            scenario: scenario.to_string(),
            millis: elapsed.as_millis(),
            written: outcome.written.len(),
            unchanged: outcome.unchanged.len(),
        }
    }

    /// The human-readable one-liner (the non-`--json` rendering).
    fn human_line(&self) -> String {
        format!(
            "[{}] regenerated in {} ms ({} written, {} unchanged)",
            self.scenario, self.millis, self.written, self.unchanged
        )
    }
}

/// Whether a single changed path should TRIGGER a regeneration (PURE — no watcher, no I/O).
///
/// `true` only when the path is a `*.go` source file that is NOT under any configured output path.
/// An output-path event (gnr8's own write) returns `false` — the loop-safety core of WATCH-02. A
/// non-`.go` file (e.g. `README.md`) returns `false`. This is unit-tested directly without a watcher.
#[must_use]
fn is_trigger_path(path: &Path, output_set: &HashSet<PathBuf>) -> bool {
    // Layer 2 of loop defense: drop anything under (or equal to) a configured output path so gnr8's
    // OWN writes never trigger regeneration, even if outputs sit inside a watched source tree.
    if is_under_any_output(path, output_set) {
        return false;
    }
    // Only supported Go source edits drive work.
    path.extension().is_some_and(|ext| ext == "go")
}

/// Whether a debounced BATCH of changed paths should trigger a regeneration (PURE).
///
/// `true` if ANY path is a trigger ([`is_trigger_path`]). A mixed batch containing both an output-path
/// event and a source `*.go` event triggers — the source edit wins, the output write is ignored.
#[must_use]
fn batch_should_regenerate(paths: &[PathBuf], output_set: &HashSet<PathBuf>) -> bool {
    paths.iter().any(|p| is_trigger_path(p, output_set))
}

/// Whether `path` is equal to, or nested under, any output path in `output_set`.
fn is_under_any_output(path: &Path, output_set: &HashSet<PathBuf>) -> bool {
    output_set.iter().any(|out| path == out || path.starts_with(out))
}

/// Canonicalize `path` to its real, symlink-resolved absolute form, falling back to `path` itself when
/// canonicalization fails (e.g. the file was just deleted/renamed — common in a watch event stream).
///
/// This MATTERS for loop safety: on macOS, `notify`/FSEvents reports the CANONICAL path
/// (`/private/var/...`) while a project root under `std::env::temp_dir()` is the non-canonical
/// `/var/...`. Without canonicalizing BOTH the output set and each incoming event path, a `starts_with`
/// comparison silently fails and gnr8's own writes would re-trigger regeneration (a WATCH-02 loop). We
/// canonicalize once when building the output set and once per event before the pure filter runs.
fn canonicalize_or_keep(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Build the absolute, CANONICALIZED output-path set the filter drops: the configured OpenAPI file, the
/// SDK dir, and every path the manifest records as gnr8-owned. Resolved against `project_root` and
/// canonicalized so the comparison matches the canonical paths `notify` reports (macOS `/private/...`
/// vs `/...`). Missing/corrupt manifest degrades to the config-derived set (loop safety never depends
/// on a readable manifest).
fn build_output_set(project_root: &Path, config: &Config) -> HashSet<PathBuf> {
    let mut set: HashSet<PathBuf> = HashSet::new();
    // The two configured output anchors (D-02): the OpenAPI artifact + the whole SDK directory. The
    // SDK dir is added as a prefix so EVERY generated SDK file under it is filtered with one entry.
    set.insert(canonicalize_or_keep(
        &project_root.join(&config.output.openapi),
    ));
    set.insert(canonicalize_or_keep(
        &project_root.join(config.output.sdk_dir.trim_end_matches('/')),
    ));

    // Belt-and-braces: also fold in every manifest-recorded path (the exact files gnr8 last wrote).
    // The manifest lives under `.gnr8/`; absent/corrupt → empty default (no panic), so loop safety
    // still holds via the config anchors above.
    if let Ok(manifest) = gnr8_core::manifest::load(&project_root.join(".gnr8")) {
        for entry in &manifest.files {
            set.insert(canonicalize_or_keep(&project_root.join(&entry.path)));
        }
    }
    set
}

/// Run the debounced, loop-safe watch loop until Ctrl-C (the I/O shell — WATCH-02 / WATCH-03).
///
/// Watches the configured SOURCE input dir(s) recursively (NEVER the output dirs — the primary loop
/// defense), debounces bursts into a single coalesced signal, and on each signal times a
/// [`lifecycle::regenerate`] call, printing a latency line (human, or a [`LatencyReport`] under `json`).
/// A `notify` error in a batch is logged to stderr and the loop CONTINUES — it never panics
/// (T-04-03-03).
///
/// ## Shutdown (W4 / A5 — `AtomicBool` stop flag set by a Ctrl-C handler)
///
/// The foreground loop runs while a shared `AtomicBool` is false and observes it via `recv_timeout`
/// ticks; it also exits on the debouncer's mpsc channel disconnect. A `ctrlc::set_handler` flips the
/// flag on SIGINT (the loop wakes on the next tick and returns `Ok(())` cleanly, dropping the debouncer
/// so the watcher stops — no orphan, no panic).
///
/// The pure-std approach (rely on the OS default SIGINT disposition) was tried first per A5/W4 and
/// proved INSUFFICIENT: the macOS FSEvents backend's run loop suppresses the default disposition, so an
/// interactive Ctrl-C did NOT terminate the process. `unsafe_code = "forbid"` rules out a hand-rolled
/// signal handler, so the RESEARCH-sanctioned fallback (`ctrlc`, pre-approved in the legitimacy audit)
/// is used — the choice is documented in the SUMMARY. The handler only flips an `AtomicBool`; the flag
/// is NOT tied to stdin, so `gnr8 watch` runs fine backgrounded / in a pipe.
///
/// # Errors
///
/// Returns an error if the Ctrl-C handler cannot be installed, the debouncer cannot be created, or the
/// source dir cannot be watched. Per-batch `regenerate` errors are logged and the loop continues (a
/// transient pipeline error must not kill a long-running watch).
pub(crate) fn run(
    project_root: &Path,
    config: &Config,
    debounce: Duration,
    json: bool,
) -> anyhow::Result<()> {
    let output_set = build_output_set(project_root, config);

    // The coalesced "a source file changed → regenerate" channel. The debouncer callback (a separate
    // thread) sends `()`; the main loop receives. A single debounced batch sends at most one signal.
    let (tx, rx) = mpsc::channel::<()>();

    // The in-process shutdown flag (W4): the receive loop runs while it is false and observes it via
    // recv_timeout ticks. Set by the Ctrl-C handler (below) and on channel disconnect. NOT tied to
    // stdin, so a backgrounded / piped `gnr8 watch` runs fine.
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        // Flip the flag on SIGINT so the foreground loop exits cleanly (W4 / A5 fallback — the default
        // disposition is suppressed by the FSEvents run loop). The handler does the minimum: a single
        // atomic store, no allocation/I-O.
        ctrlc::set_handler(move || stop.store(true, Ordering::Relaxed))
            .context("failed to install the Ctrl-C handler")?;
    }

    let filter_set = output_set.clone();
    let mut debouncer = new_debouncer(debounce, None, move |result: DebounceEventResult| {
        match result {
            Ok(events) => {
                // Flatten every changed path in the debounced batch, CANONICALIZE each (so it matches
                // the canonicalized output set — macOS `/private/...` vs `/...`, the loop-safety fix),
                // then apply the PURE filter. A batch that touches only output paths (gnr8's own writes)
                // sends NOTHING → loop-safe.
                let paths: Vec<PathBuf> = events
                    .iter()
                    .flat_map(|ev| ev.paths.iter())
                    .map(|p| canonicalize_or_keep(p))
                    .collect();
                if batch_should_regenerate(&paths, &filter_set) {
                    // Coalesce: one signal per qualifying batch. A closed receiver (loop exiting) is
                    // a benign send error — ignore it rather than panic.
                    let _ = tx.send(());
                }
            }
            // A watcher error must NOT kill the loop (T-04-03-03): log and keep watching.
            Err(errors) => {
                for err in errors {
                    eprintln!("watch: notify error (continuing): {err}");
                }
            }
        }
    })
    .context("failed to create the file-system debouncer")?;

    // Watch the configured SOURCE dirs only — NEVER output dirs (primary loop defense, WATCH-02). At
    // least one input is guaranteed by config validation in `regenerate`; watch each that exists.
    for input in &config.inputs {
        let dir = project_root.join(input);
        debouncer
            .watch(&dir, RecursiveMode::Recursive)
            .with_context(|| format!("failed to watch source dir {}", dir.display()))?;
    }

    // The receive loop. recv_timeout wakes periodically so the stop flag is observed promptly even when
    // no events arrive. On a signal: time one regeneration and print the latency line.
    let poll = Duration::from_millis(200);
    while !stop.load(Ordering::Relaxed) {
        match rx.recv_timeout(poll) {
            Ok(()) => {
                // Drain any extra signals that piled up so a burst of debounced batches collapses into
                // a single regeneration (further coalescing on top of the debouncer).
                while rx.try_recv().is_ok() {}
                regenerate_and_report("single-file-edit", project_root, config, json);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {} // tick — re-check the stop flag.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The debouncer's sender was dropped → no more events can arrive → exit cleanly.
                stop.store(true, Ordering::Relaxed);
            }
        }
    }

    // Dropping `debouncer` here stops the watcher thread cleanly (no orphaned watcher, T-04-03 lifecycle).
    drop(debouncer);
    // A human status line only; under `--json` the stream stays pure latency records (no stray text).
    if !json {
        println!("watch: stopped.");
    }
    Ok(())
}

/// Time one [`lifecycle::regenerate`] and print its latency line (human or `--json`). A regeneration
/// error is logged to stderr and the loop continues (a transient pipeline failure must not kill watch).
fn regenerate_and_report(scenario: &str, project_root: &Path, config: &Config, json: bool) {
    let t0 = Instant::now();
    match lifecycle::regenerate(project_root, config, false) {
        Ok(outcome) => {
            let elapsed = t0.elapsed();
            // Surface protected (user-edited) files so the "no silent clobber" guarantee stays visible.
            for path in &outcome.skipped {
                eprintln!(
                    "warning: {path} was hand-edited since gnr8 last wrote it — skipped (use `gnr8 generate --force` to overwrite)"
                );
            }
            let report = LatencyReport::from_outcome(scenario, elapsed, &outcome);
            print_report(&report, json);
        }
        Err(err) => eprintln!("watch: regeneration failed (continuing): {err}"),
    }
}

/// Render a [`LatencyReport`] as a human line or, under `--json`, a single JSON record per line.
fn print_report(report: &LatencyReport, json: bool) {
    if json {
        match serde_json::to_string(report) {
            Ok(line) => println!("{line}"),
            Err(err) => eprintln!("watch: failed to serialize latency report: {err}"),
        }
    } else {
        println!("{}", report.human_line());
    }
}

/// Run the COLD regeneration once at watch startup (so the cold-latency scenario is measured and the
/// outputs are current before the loop begins) and print its latency line.
///
/// # Errors
///
/// Propagates a `regenerate` error (e.g. missing Go toolchain) so startup fails loudly via the anyhow
/// boundary rather than entering a watch loop with stale/absent outputs.
pub(crate) fn cold_regenerate(
    project_root: &Path,
    config: &Config,
    json: bool,
) -> anyhow::Result<()> {
    let t0 = Instant::now();
    let outcome = lifecycle::regenerate(project_root, config, false)
        .context("initial (cold) regeneration failed")?;
    let elapsed = t0.elapsed();
    for path in &outcome.skipped {
        eprintln!(
            "warning: {path} was hand-edited since gnr8 last wrote it — skipped (use `gnr8 generate --force` to overwrite)"
        );
    }
    let report = LatencyReport::from_outcome("cold", elapsed, &outcome);
    print_report(&report, json);
    Ok(())
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4); scope the allow to
    // this module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{batch_should_regenerate, is_trigger_path, GenerateOutcome, LatencyReport};
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::Duration;

    /// The output set used across the pure-filter tests: an OpenAPI file + the SDK directory.
    fn output_set() -> HashSet<PathBuf> {
        let mut set = HashSet::new();
        set.insert(PathBuf::from("/proj/openapi.yaml"));
        set.insert(PathBuf::from("/proj/sdk")); // a directory prefix — every file under it is filtered.
        set
    }

    #[test]
    fn output_paths_filtered() {
        let out = output_set();
        // The OpenAPI artifact itself (an exact output path) is gnr8's own write → NOT a trigger.
        assert!(!is_trigger_path(&PathBuf::from("/proj/openapi.yaml"), &out));
        // A generated SDK file UNDER the sdk dir is gnr8's own write → NOT a trigger (loop-safe), even
        // though it ends in `.go`.
        assert!(!is_trigger_path(&PathBuf::from("/proj/sdk/client.go"), &out));
        assert!(!is_trigger_path(&PathBuf::from("/proj/sdk/models.go"), &out));
    }

    #[test]
    fn go_source_triggers() {
        let out = output_set();
        // A `.go` source edit OUTSIDE the output paths → a trigger.
        assert!(is_trigger_path(&PathBuf::from("/proj/handlers/goal.go"), &out));
        assert!(is_trigger_path(&PathBuf::from("/proj/main.go"), &out));
    }

    #[test]
    fn non_go_ignored() {
        let out = output_set();
        // A non-`.go` source-tree edit (docs, config) → NOT a trigger.
        assert!(!is_trigger_path(&PathBuf::from("/proj/README.md"), &out));
        assert!(!is_trigger_path(&PathBuf::from("/proj/go.mod"), &out));
        // No extension at all → NOT a trigger.
        assert!(!is_trigger_path(&PathBuf::from("/proj/Makefile"), &out));
    }

    #[test]
    fn source_wins_over_output() {
        let out = output_set();
        // A debounced batch containing BOTH gnr8's own output write AND a real source edit triggers:
        // the source edit wins; the output write is ignored (loop-safe).
        let batch = vec![
            PathBuf::from("/proj/sdk/client.go"), // gnr8's own write — dropped.
            PathBuf::from("/proj/openapi.yaml"),  // gnr8's own write — dropped.
            PathBuf::from("/proj/handlers/goal.go"), // a real source edit — wins.
        ];
        assert!(batch_should_regenerate(&batch, &out));

        // A batch of ONLY output writes must NOT trigger (the no-loop guarantee).
        let only_outputs = vec![
            PathBuf::from("/proj/sdk/client.go"),
            PathBuf::from("/proj/sdk/models.go"),
            PathBuf::from("/proj/openapi.yaml"),
        ];
        assert!(!batch_should_regenerate(&only_outputs, &out));
    }

    #[test]
    fn latency_report_json_field_set() {
        // The `--json` latency record must expose exactly the documented WATCH-03 field set so Phase
        // 5's benchmark tooling can rely on it (plan-check INFO-01).
        let outcome = GenerateOutcome {
            written: vec!["openapi.yaml".to_string(), "sdk/client.go".to_string()],
            unchanged: vec!["sdk/models.go".to_string()],
            skipped: vec![],
        };
        let report =
            LatencyReport::from_outcome("single-file-edit", Duration::from_millis(42), &outcome);
        let value: serde_json::Value = serde_json::to_value(&report).unwrap();
        let obj = value.as_object().expect("latency report serializes to a JSON object");

        // Exactly these four keys, no more, no fewer.
        let keys: HashSet<&str> = obj.keys().map(String::as_str).collect();
        let expected: HashSet<&str> =
            ["scenario", "millis", "written", "unchanged"].into_iter().collect();
        assert_eq!(keys, expected, "latency --json field set drifted");

        assert_eq!(obj["scenario"], serde_json::json!("single-file-edit"));
        assert_eq!(obj["millis"], serde_json::json!(42));
        assert_eq!(obj["written"], serde_json::json!(2));
        assert_eq!(obj["unchanged"], serde_json::json!(1));
    }
}
