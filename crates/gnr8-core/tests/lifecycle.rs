//! Phase-4 lifecycle core tests (the shared file 04-02 extends): the idempotent `.gnr8/`
//! scaffold (WS-01/WS-02) and the typed TOML `Config` surface (WS-03).
//!
//! Tests are hermetic — each creates a UNIQUE temp subdir under `std::env::temp_dir()`
//! (PID + nanosecond timestamp, no user-supplied path component, mirrors `tests/sdk_compile.rs`
//! and the zero-`tempfile`-dependency precedent, threat T-04-01-01). No state escapes the temp dir.

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4 + ch.5); scope the
// allow to this test target so the workspace-wide RUST-04 deny stays intact for production code.
// `doc_markdown` is allowed for the acronym-dense prose doc comments (deny_unknown_fields, WS-03, ...).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown
)]

use std::path::PathBuf;

use gnr8_core::CoreError;

/// Create a UNIQUE temp subdir under `std::env::temp_dir()` (PID + nanosecond timestamp — no
/// user-supplied path component). No `tempfile` crate (mirrors `tests/sdk_compile.rs`).
fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-lifecycle-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

// ---------------------------------------------------------------------------
// WS-01 / WS-02 — workspace::init idempotent scaffold
// ---------------------------------------------------------------------------

/// WS-01: `init` on a fresh dir creates `.gnr8/`, `.gnr8/cache/`, and writes both `config.toml`
/// and `.gitignore`; `InitOutcome.created` lists exactly those two files, `skipped` is empty.
#[test]
fn init_scaffolds_workspace() {
    let root = unique_temp_dir("scaffold");

    let outcome = gnr8_core::workspace::init(&root).expect("init on a fresh dir must succeed");

    let gnr8 = root.join(".gnr8");
    assert!(gnr8.is_dir(), ".gnr8/ must be created");
    assert!(gnr8.join("cache").is_dir(), ".gnr8/cache/ must be created");
    assert!(
        gnr8.join("config.toml").is_file(),
        ".gnr8/config.toml must be written"
    );
    assert!(
        gnr8.join(".gitignore").is_file(),
        ".gnr8/.gitignore must be written"
    );

    assert!(
        outcome.created.iter().any(|p| p.contains("config.toml")),
        "created must list config.toml, got {:?}",
        outcome.created
    );
    assert!(
        outcome.created.iter().any(|p| p.contains(".gitignore")),
        "created must list .gitignore, got {:?}",
        outcome.created
    );
    assert!(
        outcome.skipped.is_empty(),
        "skipped must be empty on a fresh init, got {:?}",
        outcome.skipped
    );

    let _ = std::fs::remove_dir_all(&root); // best-effort cleanup
}

/// WS-01 / D-01: re-running `init` after a user edits `config.toml` leaves the file byte-identical
/// to the edit (never clobbered); the second `InitOutcome.skipped` lists both files, `created` empty.
#[test]
fn init_is_idempotent() {
    let root = unique_temp_dir("idempotent");

    // First init writes the defaults.
    let first = gnr8_core::workspace::init(&root).expect("first init must succeed");
    assert!(!first.created.is_empty(), "first init must create files");

    // User edits config.toml — this content must survive a second init.
    let config_path = root.join(".gnr8").join("config.toml");
    let user_edit = b"inputs = [\"./internal\"]\n\n[output]\nopenapi = \"api.yaml\"\nsdk_dir = \"client\"\ngo_module = \"example.com/edited/sdk\"\n";
    std::fs::write(&config_path, user_edit).expect("user edits config.toml");

    // Second init must NOT clobber.
    let second = gnr8_core::workspace::init(&root).expect("second init must succeed");

    let on_disk = std::fs::read(&config_path).expect("read config.toml after second init");
    assert_eq!(
        on_disk, user_edit,
        "second init must preserve the user's config.toml edit byte-for-byte (D-01)"
    );

    assert!(
        second.created.is_empty(),
        "second init must create nothing, got {:?}",
        second.created
    );
    assert!(
        second.skipped.iter().any(|p| p.contains("config.toml")),
        "second init must skip config.toml, got {:?}",
        second.skipped
    );
    assert!(
        second.skipped.iter().any(|p| p.contains(".gitignore")),
        "second init must skip .gitignore, got {:?}",
        second.skipped
    );

    let _ = std::fs::remove_dir_all(&root); // best-effort cleanup
}

/// WS-02: the written `.gnr8/.gitignore` body ignores the lifecycle cache (`/cache/`) while keeping
/// `config.toml` checked in (the body never names config.toml).
#[test]
fn gitignore_splits_lifecycle() {
    let root = unique_temp_dir("gitignore");
    gnr8_core::workspace::init(&root).expect("init must succeed");

    let body = std::fs::read_to_string(root.join(".gnr8").join(".gitignore"))
        .expect("read .gnr8/.gitignore");

    assert!(
        body.contains("/cache/"),
        ".gitignore must ignore the lifecycle cache dir, got:\n{body}"
    );
    assert!(
        !body.contains("config.toml"),
        ".gitignore must NOT ignore config.toml (it is checked in), got:\n{body}"
    );

    // The exported constant is the source of truth and matches what was written.
    assert_eq!(
        body,
        gnr8_core::workspace::GITIGNORE_BODY,
        "written .gitignore must equal the GITIGNORE_BODY constant"
    );

    let _ = std::fs::remove_dir_all(&root); // best-effort cleanup
}

// ---------------------------------------------------------------------------
// WS-03 — config::parse typed TOML surface (added in Task 3)
// ---------------------------------------------------------------------------

/// WS-03: the default body `init` writes round-trips through the config parser with the documented
/// knobs (the contract between the workspace default and the config layer).
#[test]
fn config_parses_default_body() {
    let config = gnr8_core::config::parse(gnr8_core::workspace::DEFAULT_CONFIG_TOML)
        .expect("DEFAULT_CONFIG_TOML must parse via the config layer");

    assert_eq!(config.inputs, vec![".".to_string()]);
    assert_eq!(config.output.openapi, "openapi.yaml");
    assert_eq!(config.output.sdk_dir, "sdk");
    assert!(
        !config.output.go_module.is_empty(),
        "go_module must be a non-empty placeholder"
    );
    assert!(
        config.naming.operations.is_empty(),
        "default body has no operation overrides"
    );
    assert!(
        config.naming.types.is_empty(),
        "default body has no type overrides"
    );
}

/// WS-03: a config with `[naming.operations]` and `[naming.types]` populates both BTreeMaps.
#[test]
fn naming_overrides_parse() {
    let src = r#"
inputs = ["."]

[output]
openapi = "openapi.yaml"
sdk_dir = "sdk"
go_module = "example.com/svc/sdk"

[naming.operations]
goalUuidPut = "UpdateGoal"

[naming.types]
CreateGoalInput = "NewGoal"
"#;

    let config = gnr8_core::config::parse(src).expect("config with naming tables must parse");

    assert_eq!(
        config.naming.operations.get("goalUuidPut"),
        Some(&"UpdateGoal".to_string())
    );
    assert_eq!(
        config.naming.types.get("CreateGoalInput"),
        Some(&"NewGoal".to_string())
    );
}

/// WS-03 / V5: an unknown top-level key is rejected with `CoreError::Config` (deny_unknown_fields),
/// never a panic (T-04-01-03).
#[test]
fn config_rejects_unknown_key() {
    let src = r#"
inputs = ["."]
bogus = 1

[output]
openapi = "openapi.yaml"
sdk_dir = "sdk"
go_module = "example.com/svc/sdk"
"#;

    let err = gnr8_core::config::parse(src).expect_err("an unknown key must be rejected");
    assert!(
        matches!(err, CoreError::Config { .. }),
        "expected CoreError::Config for an unknown key, got {err:?}"
    );
}
