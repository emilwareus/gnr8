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
//! ## The `gnr8` dependency: one selected resource source
//!
//! `init` emits a path dependency to `crates/gnr8-core` under the single resource root selected for
//! this build. Development builds select the repository root fixed at compile time; release builds
//! select the archive's `share/gnr8` root. A missing source is an error, not a registry substitution.
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
/// the generated-header post-process) and hands it to [`crate::runner::run`]. The user edits it to
/// adapt parsing + generation; `gnr8 generate` compiles and runs it.
fn main_rs_body(source: SourcePreset, sdk: SdkPreset) -> String {
    format!(
        r#"//! This file IS your gnr8 configuration — edit it to adapt parsing + generation.
//! `gnr8 generate` compiles and runs it.
//!
//! It is an ordinary Rust binary that composes a `Pipeline` and hands it to the gnr8 runner. The
//! runner parses argv (`__emit` / `__inspect`) and prints a JSON bundle on stdout; the `gnr8` host
//! runs this crate for you, then owns writing the files (ownership manifest, no-op skip, edit
//! protection). Adapting = ordinary Rust: change an argument, add a `.transform(...)`, write your own
//! `Source`/`Target`/`Transform`, or wrap a built-in.

use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {{
    gnr8::runner::run(
        Pipeline::new()
            .source({source_stage})
            .transform(SetBasePath::new("/"))
            .transform(SetTitle::new("API"))
            // .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            // .transform(RenameOperation::new("listGoals", "List"))
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

/// The `gnr8` dependency line for a `.gnr8/Cargo.toml` scaffolded under `root`.
///
/// Production scaffolds pin the published crate version so the same `Cargo.toml` works on every
/// machine. Sidecar resources still come from the packaged CLI install (via `GNR8_RESOURCE_DIR` or
/// executable-relative discovery); the crates.io package supplies the Rust API only.
///
/// When developing gnr8 itself inside this repository, prefer a path dependency so `init` stays
/// offline and tracks the local workspace.
fn core_dependency_line() -> String {
    core_dependency_line_for(env!("CARGO_PKG_VERSION"), in_repo_checkout().as_deref())
}

fn core_dependency_line_for(version: &str, prefer_path: Option<&Path>) -> String {
    if let Some(resource_root) = prefer_path {
        let candidate = resource_root.join("crates").join("gnr8-core");
        if candidate.join("Cargo.toml").is_file() {
            if let Ok(path) = std::fs::canonicalize(&candidate) {
                return format!("gnr8 = {{ path = {:?} }}", path.to_string_lossy());
            }
        }
    }
    format!("gnr8 = \"={version}\"")
}

/// True only when this binary was built from the gnr8 git checkout (not a packaged archive).
///
/// Packaged archives also contain `crates/gnr8-core`, so presence of that crate alone is not enough.
fn in_repo_checkout() -> Option<PathBuf> {
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let marker = root.join("crates").join("gnr8-core").join("Cargo.toml");
    let git = root.join(".git");
    if marker.is_file() && git.exists() {
        Some(root)
    } else {
        None
    }
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

    use super::{core_dependency_line_for, crate_name_for};
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
        let line = core_dependency_line_for("0.1.22", None);
        assert_eq!(line, "gnr8 = \"=0.1.22\"");
    }

    #[test]
    fn in_repo_init_prefers_path_dependency_when_resources_exist() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let line = core_dependency_line_for("0.1.22", Some(root.as_path()));
        assert!(
            line.contains("path ="),
            "expected path dependency in-repo, got: {line}"
        );
    }
}
