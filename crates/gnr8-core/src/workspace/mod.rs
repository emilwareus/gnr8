//! The `.gnr8/` workspace lifecycle: idempotent `init` scaffold + the checked-in/ignored split
//! (WS-01, WS-02, D-01, D-02).
//!
//! `init` creates a project-local `.gnr8/` directory holding the checked-in PoC config
//! (`config.toml`) and an auto-written `.gnr8/.gitignore` that ignores the git-ignored lifecycle
//! subtree (`cache/`). The generated SDK/OpenAPI *outputs* live OUTSIDE `.gnr8/` at user-configured
//! project paths (D-02) and are intentionally committed by the user — they are NOT scaffolded here.
//!
//! Idempotency (D-01): every workspace file is written *only if absent*. Re-running `init` over an
//! edited `config.toml` preserves the user's edits byte-for-byte and reports the file as `skipped`,
//! never an error and never an overwrite. The write-if-absent guarantee uses
//! `OpenOptions::create_new(true)`, which atomically fails with [`std::io::ErrorKind::AlreadyExists`]
//! if the file appears between the check and the write (TOCTOU-safe, threat T-04-01-01) — no path is
//! derived from user input (the `.gnr8/` subtree is fixed), so there is no traversal surface.

// These docs are user-facing prose dense with proper nouns/acronyms (PoC, OpenAPI, TOCTOU, ...);
// backticking them would hurt readability. Allow `doc_markdown` module-wide (skill ch.2.4, mirrors
// the scoped allow in gnr8/src/cli.rs).
#![allow(clippy::doc_markdown)]

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::CoreError;

/// The exact body `init` writes to `.gnr8/.gitignore` (WS-02 / D-01).
///
/// The `.gitignore` lives *inside* `.gnr8/`, so its patterns are relative to `.gnr8/`. A leading
/// slash anchors `/cache/` to this directory: it hides the ownership manifest + any future cache
/// while keeping `config.toml` and the `.gitignore` itself checked in. This realizes the
/// "automatic checked-in / git-ignored split" with a single file. Generated outputs
/// (`openapi.yaml`, `sdk/`) live OUTSIDE `.gnr8/` (D-02) and are intentionally committed.
pub const GITIGNORE_BODY: &str = "\
# gnr8 lifecycle state — regenerated, do not commit.
/cache/
";

/// The default `config.toml` body `init` writes (checked in, WS-03 / D-03).
///
/// This TOML surface is an explicit **PoC code-as-config stand-in**, NOT the long-term UX: PROJECT
/// scopes the real customization ("through code") to v2 (ADV-02). The body carries only the
/// documented knobs — source inputs, OpenAPI/SDK output paths + Go module path, and optional
/// commented-out naming overrides — and round-trips through [`crate::config::parse`] (the
/// `deny_unknown_fields` config parser accepts exactly these keys).
pub const DEFAULT_CONFIG_TOML: &str = "\
# gnr8 PoC configuration — a code-as-config STAND-IN, not the long-term UX (see docs / D-03).
# Programmatic (\"through code\") customization of routing recognition / transport / emitters is a
# documented v2 direction (ADV-02) and is deliberately NOT a knob here.
inputs = [\".\"]                              # Go source dir(s) to analyze (project-relative)

[output]
openapi   = \"openapi.yaml\"                  # OpenAPI artifact path (project-relative)
sdk_dir   = \"sdk\"                           # generated Go SDK directory
go_module = \"example.com/yourservice/sdk\"   # Go module path for the generated SDK

# [naming.operations]                        # optional: remap operation ids, e.g.
# goalUuidPut = \"UpdateGoal\"
# [naming.types]                             # optional: remap generated type names, e.g.
# CreateGoalInput = \"NewGoal\"
";

/// The outcome of [`init`], so the CLI can report created vs already-present files without
/// re-reading disk. Paths are relative to the project root (e.g. `.gnr8/config.toml`).
#[derive(Debug, Default)]
pub struct InitOutcome {
    /// Relative paths newly written by this `init` invocation.
    pub created: Vec<String>,
    /// Relative paths that already existed and were left untouched (idempotent skip, D-01).
    pub skipped: Vec<String>,
}

/// Scaffold the `.gnr8/` workspace idempotently under `root`.
///
/// Creates `.gnr8/cache/` (mkdir -p is idempotent by nature), then writes `.gnr8/config.toml` and
/// `.gnr8/.gitignore` *only if absent*. An already-initialized workspace is a successful no-op
/// (files recorded in [`InitOutcome::skipped`]), never an error and never an overwrite (D-01).
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if `.gnr8/cache/` cannot be created or a workspace file cannot
/// be written for any reason other than already existing. No production panic (RUST-04).
pub fn init(root: &Path) -> Result<InitOutcome, CoreError> {
    let gnr8 = root.join(".gnr8");
    let cache = gnr8.join("cache");
    std::fs::create_dir_all(&cache).map_err(|e| CoreError::Workspace {
        message: format!("failed to create {}: {e}", cache.display()),
    })?;

    let mut outcome = InitOutcome::default();
    write_if_absent(root, &gnr8.join("config.toml"), DEFAULT_CONFIG_TOML, &mut outcome)?;
    write_if_absent(root, &gnr8.join(".gitignore"), GITIGNORE_BODY, &mut outcome)?;
    Ok(outcome)
}

/// Write `body` to `path` only if it does not already exist; record the relative path in
/// `out.created` (newly written) or `out.skipped` (already present — left untouched).
///
/// Uses `OpenOptions::create_new(true)` for the atomic write-if-absent guarantee: on
/// [`std::io::ErrorKind::AlreadyExists`] the existing file is preserved (idempotent, D-01); any
/// other I/O error becomes [`CoreError::Workspace`]. Never clobbers a user's edits.
fn write_if_absent(
    root: &Path,
    path: &Path,
    body: &str,
    out: &mut InitOutcome,
) -> Result<(), CoreError> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(body.as_bytes()).map_err(|e| CoreError::Workspace {
                message: format!("failed to write {}: {e}", path.display()),
            })?;
            out.created.push(relative(root, path));
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            out.skipped.push(relative(root, path));
            Ok(())
        }
        Err(e) => Err(CoreError::Workspace {
            message: format!("failed to write {}: {e}", path.display()),
        }),
    }
}

/// Render `path` relative to `root` for reporting; fall back to the full path if it is not a
/// descendant of `root` (defensive — `init` only ever passes paths under `root`).
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map_or_else(|_| path.to_path_buf(), PathBuf::from)
        .display()
        .to_string()
}
