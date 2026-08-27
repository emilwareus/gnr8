//! The `.gnr8/` workspace lifecycle: idempotent `init` scaffold of the MANDATORY code-as-config crate
//! (WS-01, WS-02, D-01, D-02).
//!
//! `gnr8 init` creates a project-local `.gnr8/` directory holding a small Rust **binary crate** that
//! depends on the public `gnr8` crate and drives the generation lifecycle. THIS CRATE IS THE CONFIG — there is no
//! TOML (`docs/code-as-config.md`). gnr8 does not run without it: every other command requires it and
//! errors with "run `gnr8 init`" when it is absent. `init` writes four files (each only if absent):
//!
//! - `.gnr8/Cargo.toml` — a standalone-workspace crate (`name = "<dir>-gnr8-gen"`, edition 2021,
//!   `publish = false`, an empty `[workspace]` table so it builds independently via `--manifest-path`,
//!   and a `gnr8` dependency).
//! - `.gnr8/src/main.rs` — the default pipeline, in code; the user edits this to adapt parsing +
//!   generation.
//! - `.gnr8/.gitignore` — ignores the git-ignored lifecycle subtree (`target/`, `cache/`).
//! - `.gnr8/README.md` — project-local instructions for agents and humans editing the pipeline.
//!
//! The generated SDK/OpenAPI *outputs* live OUTSIDE `.gnr8/` at the paths the pipeline's targets
//! declare (D-02) and are intentionally committed by the user — they are NOT scaffolded here.
//!
//! ## The `gnr8` dependency: one compile-time choice
//!
//! The scaffolded crate depends on exactly one package: the thin `gnr8` SDK. It never depends on the
//! host engine, so upgrading gnr8 does not recompile a code generator inside the project.
//!
//! A packaged build emits `gnr8 = "=<version>"` — the exact published crate — so the scaffolded
//! `Cargo.toml` is portable and can be committed. A build from this repository emits a path
//! dependency on the local `crates/gnr8-sdk` instead, so developing gnr8 itself stays offline.
//! `scripts/package-release.sh` fixes the choice at compile time via `GNR8_PACKAGED_RELEASE`; it is
//! never inferred from the runtime filesystem. See [`core_dependency_line`].
//!
//! Idempotency (D-01): every workspace file is written *only if absent*, via
//! `OpenOptions::create_new(true)` — atomically failing with [`std::io::ErrorKind::AlreadyExists`] if
//! the file appears between the check and the write (TOCTOU-safe, threat T-04-01-01). Re-running `init`
//! over an edited `src/main.rs` preserves the user's edits byte-for-byte and reports the file as
//! `skipped`. The `.gnr8/` subtree is fixed (no path is derived from user input), so there is no
//! traversal surface; the only user-derived value is the sanitized crate name written INTO Cargo.toml.

// These docs are user-facing prose dense with proper nouns/acronyms (PoC, OpenAPI, TOCTOU, Cargo, ...);
// backticking them would hurt readability. Allow `doc_markdown` module-wide (skill ch.2.4, mirrors the
// scoped allow in gnr8/src/cli.rs).
#![allow(clippy::doc_markdown)]

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::CoreError;

/// The exact body `init` writes to `.gnr8/.gitignore` (WS-02 / D-01).
///
/// The `.gitignore` lives *inside* `.gnr8/`, so its patterns are relative to `.gnr8/`. Leading slashes
/// anchor `/target/` and `/cache/` to this directory: they hide the Rust build output of the generation
/// crate and the ownership-manifest cache while keeping `Cargo.toml`, `src/`, and the `.gitignore`
/// itself checked in. Generated outputs (`openapi.yaml`, `sdk/`) live OUTSIDE `.gnr8/` (D-02) and are
/// intentionally committed.
pub const GITIGNORE_BODY: &str = "\
# gnr8 generation crate build output + lifecycle state — regenerated, do not commit.
/target/
/cache/
";

/// The outcome of [`init`], so the CLI can report created vs already-present files without
/// re-reading disk. Paths are relative to the project root (e.g. `.gnr8/Cargo.toml`).
#[derive(Debug, Default)]
pub struct InitOutcome {
    /// Relative paths newly written by this `init` invocation.
    pub created: Vec<String>,
    /// Relative paths that already existed and were left untouched (idempotent skip, D-01).
    pub skipped: Vec<String>,
}

/// Source frontend preset for the scaffolded `.gnr8/src/main.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePreset {
    /// Go + Gin source extraction.
    GoGin,
    /// Python FastAPI source extraction.
    FastApi,
    /// Python Flask typed-envelope source extraction.
    Flask,
    /// TypeScript NestJS class-DTO source extraction.
    NestJs,
}

impl SourcePreset {
    fn stage(self) -> &'static str {
        match self {
            Self::GoGin => "GoGin::new().inputs([\".\"])",
            Self::FastApi => "FastApi::new().inputs([\".\"])",
            Self::Flask => "Flask::new().inputs([\".\"])",
            Self::NestJs => "NestJs::new().inputs([\"src\"])",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::GoGin => "Go + Gin",
            Self::FastApi => "Python FastAPI",
            Self::Flask => "Python Flask typed-envelope",
            Self::NestJs => "TypeScript NestJS class DTOs",
        }
    }

    fn toolchain(self) -> &'static str {
        match self {
            Self::GoGin => "go",
            Self::FastApi | Self::Flask => "python3",
            Self::NestJs => "node plus the target project's own typescript package",
        }
    }
}

/// SDK target preset for the scaffolded `.gnr8/src/main.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkPreset {
    /// Dependency-free Go SDK.
    Go,
    /// Python SDK.
    Python,
    /// Dependency-free TypeScript SDK.
    TypeScript,
}

impl SdkPreset {
    fn stage(self) -> &'static str {
        match self {
            Self::Go => "GoSdk::new().module(\"example.com/yourservice/sdk\").to(\"sdk\")",
            Self::Python => "PySdk::new().module(\"example.com/yourservice/sdk\").to(\"sdk\")",
            Self::TypeScript => "TsSdk::new().module(\"example.com/yourservice/sdk\").to(\"sdk\")",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Go => "Go",
            Self::Python => "Python",
            Self::TypeScript => "TypeScript",
        }
    }
}

/// Scaffold the mandatory `.gnr8/` code-as-config crate idempotently under `root`.
///
/// Creates `.gnr8/src/` (mkdir -p is idempotent), then writes `.gnr8/Cargo.toml`, `.gnr8/src/main.rs`,
/// and `.gnr8/.gitignore` *only if absent*. An already-initialized workspace is a successful no-op
/// (files recorded in [`InitOutcome::skipped`]), never an error and never an overwrite (D-01).
///
/// The crate name is `<dirname>-gnr8-gen` where `<dirname>` is `root`'s final component sanitized to a
/// valid Cargo package name; the `gnr8` dependency is a path dep into the selected resource root.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if `.gnr8/src/` cannot be created or a workspace file cannot be
/// written for any reason other than already existing. No production panic (RUST-04).
pub fn init(root: &Path) -> Result<InitOutcome, CoreError> {
    init_with_presets(root, SourcePreset::GoGin, SdkPreset::Go)
}

/// Scaffold the mandatory `.gnr8/` code-as-config crate with explicit source and SDK presets.
///
/// Existing files are still preserved byte-for-byte. Presets only affect files that do not exist yet.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] on filesystem failures.
pub fn init_with_presets(
    root: &Path,
    source: SourcePreset,
    sdk: SdkPreset,
) -> Result<InitOutcome, CoreError> {
    let gnr8 = root.join(".gnr8");
    let src = gnr8.join("src");
    std::fs::create_dir_all(&src).map_err(|e| CoreError::Workspace {
        message: format!("failed to create {}: {e}", src.display()),
    })?;

    let crate_name = crate_name_for(root);
    let core_dep = core_dependency_line();
    let cargo_toml = cargo_toml_body(&crate_name, &core_dep);
    let main_rs = main_rs_body(source, sdk);
    let readme = readme_body(source, sdk);

    let mut outcome = InitOutcome::default();
    write_if_absent(root, &gnr8.join("Cargo.toml"), &cargo_toml, &mut outcome)?;
    write_if_absent(root, &src.join("main.rs"), &main_rs, &mut outcome)?;
    write_if_absent(root, &gnr8.join(".gitignore"), GITIGNORE_BODY, &mut outcome)?;
    write_if_absent(root, &gnr8.join("README.md"), &readme, &mut outcome)?;
    Ok(outcome)
}

/// The scaffolded `.gnr8/src/main.rs` — the default generation lifecycle, in code (D-03).
///
/// This file IS the config: it composes a [`crate::sdk::Pipeline`] equivalent to the old default TOML
/// (one Go+Gin source, a root base path, an `API` title, an OpenAPI 3.1 target, a Go SDK target, and
/// the generated-header post-process) and hands it to `gnr8::worker::run`. The user edits it to
/// adapt parsing + generation; `gnr8 generate` compiles and runs it.
fn main_rs_body(source: SourcePreset, sdk: SdkPreset) -> String {
    format!(
        r#"//! This file IS your gnr8 configuration — edit it to adapt parsing + generation.
//! `gnr8 generate` compiles it once, then runs it.
//!
//! It is an ordinary Rust binary that composes a `Pipeline` and hands it to the gnr8 worker runtime.
//! The built-in stages below are DECLARATIONS: the installed `gnr8` host executes them, so none of
//! the extraction or SDK-emission machinery is compiled into this crate. Your own stages — wrapped
//! in `Custom(...)` — run right here, against the graph the host sends over.
//!
//! Adapting = ordinary Rust: change an argument, add a `.transform(...)`, or write your own
//! `Source`/`Transform`/`Target`/`PostProcess` and compose it with `Custom(...)`.

use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {{
    gnr8::worker::run(
        Pipeline::new()
            .source({source_stage})
            .transform(SetBasePath::new("/"))
            .transform(SetTitle::new("API"))
            // .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            // .transform(RenameOperation::new("listGoals", "List"))
            // .transform(Custom(MyOwnTransform))   // <- your Rust runs in this process
            .target(OpenApi31::new().to("openapi.yaml"))
            .target({sdk_stage})
            .post(Header::generated()),
    )
}}
"#,
        source_stage = source.stage(),
        sdk_stage = sdk.stage()
    )
}

fn readme_body(source: SourcePreset, sdk: SdkPreset) -> String {
    format!(
        "# gnr8 generation workspace\n\n\
         This directory is the project-local gnr8 configuration. Agents and humans should edit \
         `src/main.rs`, then run `gnr8 generate` from the project root.\n\n\
         ## Current preset\n\n\
         - Source: {source}\n\
         - SDK target: {sdk}\n\
         - Required source toolchain: {toolchain}\n\n\
         ## Commands\n\n\
         ```bash\n\
         gnr8 generate      # compile and run .gnr8/src/main.rs, then write outputs\n\
         gnr8 check         # fail if generated outputs are stale or user-edited\n\
         gnr8 doctor        # summarize toolchain, pipeline, diagnostics, and drift\n\
         gnr8 guide         # print the basic agent guide and available scenario guides\n\
         gnr8 guide <topic> # print a concrete scenario guide\n\
         ```\n\n\
         Scenario topics: `go-gin-to-python-typescript`, `python-apis-to-python-sdk`, \
         `nestjs-to-typescript-sdk`.\n\n\
         ## Editing `src/main.rs`\n\n\
         The `Pipeline` is the configuration. Change the `Source` to select the service frontend, \
         transforms to set metadata such as title/base path/security, and targets to choose generated \
         artifacts.\n\n\
         Built-in stages are declarations the installed `gnr8` host runs. Your own \
         `Source`/`Transform`/`Target`/`PostProcess` implementations run in this crate and are \
         composed with `Custom(...)`.\n\n\
         Common edits:\n\n\
         ```rust\n\
         .transform(SetBasePath::new(\"/api\"))\n\
         .transform(SetTitle::new(\"Public API\"))\n\
         .transform(ApplySecurity::api_key(\"ApiKeyAuth\", \"X-API-Key\"))\n\
         .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n\
         ```\n\n\
         Generated SDKs include their own README/reference files under the SDK output directory.\n",
        source = source.label(),
        sdk = sdk.label(),
        toolchain = source.toolchain()
    )
}

/// Build the `.gnr8/Cargo.toml` body for `crate_name` with the given `gnr8` `dependency` line.
///
/// A standalone-workspace crate (the empty `[workspace]` table makes it its own workspace root so it
/// builds independently via `cargo run --manifest-path .gnr8/Cargo.toml`), `publish = false` (it is a
/// project-local tool, never published), edition 2021 (matches the gnr8 workspace).
fn cargo_toml_body(crate_name: &str, dependency: &str) -> String {
    format!(
        "# gnr8 generation crate — this crate IS your config (edit src/main.rs). Built + run by `gnr8`.\n\
         [package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [dependencies]\n\
         {dependency}\n\
         \n\
         # Empty [workspace] table → this crate is its own workspace root, so `gnr8` can build it\n\
         # standalone via `cargo run --manifest-path .gnr8/Cargo.toml` regardless of any parent workspace.\n\
         [workspace]\n"
    )
}

/// The `gnr8` dependency line for a scaffolded `.gnr8/Cargo.toml`.
///
/// A packaged build pins the published crate version, so the generated `Cargo.toml` is portable:
/// committing it does not tie teammates to one machine's install prefix. crates.io supplies the thin
/// SDK only — extraction sidecars and every generator live in the installed CLI.
///
/// A build from this repository instead points at the local workspace, so developing gnr8 itself
/// stays offline and tracks uncommitted changes to `gnr8-core`.
///
/// Which of the two applies is fixed **at compile time** by `GNR8_PACKAGED_RELEASE`, which
/// `scripts/package-release.sh` sets when it builds an archive. It is deliberately not inferred from
/// the runtime filesystem: probing for a `.git` next to the compile-time manifest directory makes a
/// released binary emit a path dependency whenever it happens to run on the machine that built it,
/// so the artifact shipped to users could never be validated by running it locally.
fn core_dependency_line() -> String {
    match option_env!("GNR8_PACKAGED_RELEASE") {
        Some(_) => version_dependency_line(env!("CARGO_PKG_VERSION")),
        None => path_dependency_line(&sdk_crate_dir()),
    }
}

/// The in-repo path of the thin SDK crate, derived from this crate's compile-time manifest dir.
fn sdk_crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        |crates| crates.join("gnr8-sdk"),
    )
}

fn version_dependency_line(version: &str) -> String {
    format!("gnr8 = \"={version}\"")
}

fn path_dependency_line(core_dir: &Path) -> String {
    let resolved = std::fs::canonicalize(core_dir).unwrap_or_else(|_| core_dir.to_path_buf());
    format!("gnr8 = {{ path = {:?} }}", resolved.to_string_lossy())
}

/// Derive the scaffolded crate name `<dirname>-gnr8-gen` from `root`'s final path component, sanitized
/// to a valid Cargo package name (lowercase ASCII alphanumerics + `-`/`_`, leading non-letter trimmed).
///
/// A Cargo package name must be non-empty and start with an alphanumeric; we keep ASCII letters/digits
/// (lower-cased) and `-`/`_`, replacing every other character (including `.`) with `-`, then trim
/// leading separators. If the component sanitizes to empty (or `root` has no final component, e.g. it is
/// the filesystem root), a stable fallback (`"gnr8-gen"`) is used so the name is always valid.
fn crate_name_for(root: &Path) -> String {
    let raw = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sanitized: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Trim leading separators so the name starts with an alphanumeric (Cargo requirement).
    let trimmed = sanitized.trim_start_matches(['-', '_']);
    if trimmed.is_empty() {
        "gnr8-gen".to_string()
    } else {
        format!("{trimmed}-gnr8-gen")
    }
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
            file.write_all(body.as_bytes())
                .map_err(|e| CoreError::Workspace {
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
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
        .display()
        .to_string()
}

/// What [`upgrade`] changed in an existing `.gnr8/` workspace.
#[derive(Debug, Default)]
pub struct UpgradeOutcome {
    /// Relative paths this call rewrote or deleted.
    pub changed: Vec<String>,
    /// Whether the manifest already named this gnr8's SDK dependency.
    pub already_current: bool,
}

/// Repoint an existing `.gnr8/Cargo.toml` at this gnr8's SDK, in place.
///
/// This is the mechanical half of moving a project onto the worker contract, and only that half:
///
/// - the `gnr8` dependency line becomes the one [`init`] would scaffold today;
/// - a dependency on a host-only engine crate is removed;
/// - `Cargo.lock` is deleted, because it pins the previous dependency tree;
/// - the worker build stamp is deleted, because it describes a binary built from the old manifest.
///
/// It never edits `src/main.rs`. Rewriting a user's Rust is not a mechanical operation, and guessing
/// at it would be worse than telling them exactly what to change — which the CLI prints.
///
/// Every other line of the manifest is preserved, including any dependency a custom stage added.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] when the manifest is missing or cannot be read/written.
pub fn upgrade(root: &Path) -> Result<UpgradeOutcome, CoreError> {
    let gnr8 = root.join(".gnr8");
    let manifest = gnr8.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| CoreError::Workspace {
        message: format!(
            "failed to read {} — run `gnr8 init` first: {e}",
            manifest.display()
        ),
    })?;

    let wanted = core_dependency_line();
    let (rewritten, replaced) = rewrite_dependency_lines(&text, &wanted);
    let mut outcome = UpgradeOutcome::default();
    if replaced {
        std::fs::write(&manifest, &rewritten).map_err(|e| CoreError::Workspace {
            message: format!("failed to write {}: {e}", manifest.display()),
        })?;
        outcome.changed.push(relative(root, &manifest));
    } else {
        outcome.already_current = true;
    }

    for stale in [
        gnr8.join("Cargo.lock"),
        gnr8.join("cache").join("worker.json"),
    ] {
        if stale.is_file() && std::fs::remove_file(&stale).is_ok() {
            outcome.changed.push(relative(root, &stale));
        }
    }
    Ok(outcome)
}

/// Replace the `gnr8` dependency line and drop host-only engine dependencies.
///
/// Line-based on purpose: a manifest may carry the user's own dependencies and comments, and a
/// parse-and-reserialize round trip would silently reformat all of it.
fn rewrite_dependency_lines(text: &str, wanted: &str) -> (String, bool) {
    let mut out = Vec::new();
    let mut changed = false;
    let mut saw_gnr8 = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if is_dependency_line(trimmed, "gnr8") {
            saw_gnr8 = true;
            if trimmed != wanted {
                changed = true;
            }
            out.push(wanted.to_string());
            continue;
        }
        if is_dependency_line(trimmed, "gnr8-engine") || is_dependency_line(trimmed, "gnr8-core") {
            changed = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !saw_gnr8 {
        if let Some(index) = out.iter().position(|line| line.trim() == "[dependencies]") {
            out.insert(index + 1, wanted.to_string());
            changed = true;
        }
    }
    let mut rendered = out.join("\n");
    if text.ends_with('\n') {
        rendered.push('\n');
    }
    (rendered, changed)
}

fn is_dependency_line(line: &str, name: &str) -> bool {
    line.strip_prefix(name)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// The path to a project's mandatory generation-crate manifest (`<root>/.gnr8/Cargo.toml`).
///
/// The host requires this to exist before running the child; a missing one is the "run `gnr8 init`"
/// error. Exposed so the binary's child-run helper resolves the manifest the same way `init` writes it.
#[must_use]
pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(".gnr8").join("Cargo.toml")
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4); scope the allow to the
    // test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{
        core_dependency_line, crate_name_for, path_dependency_line, version_dependency_line,
    };
    use std::path::Path;

    #[test]
    fn crate_name_sanitizes_dir_to_valid_cargo_name() {
        assert_eq!(
            crate_name_for(Path::new("/x/bookstore")),
            "bookstore-gnr8-gen"
        );
        // Dots and uppercase are normalized.
        assert_eq!(
            crate_name_for(Path::new("/x/My.Service.v2")),
            "my-service-v2-gnr8-gen"
        );
        // Leading separators are trimmed so the name starts with an alphanumeric.
        assert_eq!(crate_name_for(Path::new("/x/_weird")), "weird-gnr8-gen");
        // A component that sanitizes to empty falls back to the stable default.
        assert_eq!(crate_name_for(Path::new("/x/---")), "gnr8-gen");
    }

    #[test]
    fn packaged_init_emits_exact_crates_io_version_pin() {
        assert_eq!(version_dependency_line("0.1.22"), "gnr8 = \"=0.1.22\"");
    }

    #[test]
    fn in_repo_init_points_at_the_local_thin_sdk_crate() {
        let line = path_dependency_line(&super::sdk_crate_dir());
        assert!(
            line.starts_with("gnr8 = { path = \"") && line.ends_with("gnr8-sdk\" }"),
            "expected a path dependency on the local SDK crate, got: {line}"
        );
    }

    /// The packaged/in-repo choice is compile-time, so a test binary — which is never built with
    /// `GNR8_PACKAGED_RELEASE` — must always take the path branch, regardless of ambient state such
    /// as whether a `.git` directory happens to exist beside the compile-time manifest dir.
    #[test]
    fn dependency_choice_is_fixed_at_compile_time_not_probed() {
        assert_eq!(
            core_dependency_line(),
            path_dependency_line(&super::sdk_crate_dir()),
            "a non-packaged build must always emit the local path dependency"
        );
    }
}
