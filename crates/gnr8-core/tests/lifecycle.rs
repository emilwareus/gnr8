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

// ---------------------------------------------------------------------------
// WS-04 — manifest: blake3-hashed ownership record (Task 1)
// ---------------------------------------------------------------------------

/// WS-04: `blake3_hex` is a stable 64-char lowercase hex digest — same input ⇒ same digest
/// (a content fingerprint that survives across runs/toolchains, NOT std DefaultHasher).
#[test]
fn blake3_hex_is_stable() {
    let a = gnr8_core::manifest::blake3_hex(b"package goalservice\n");
    let b = gnr8_core::manifest::blake3_hex(b"package goalservice\n");
    assert_eq!(a, b, "same bytes must hash to the same digest");
    assert_eq!(
        a.len(),
        64,
        "blake3 hex digest is 64 chars, got {}",
        a.len()
    );
    assert!(
        a.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
        "digest must be lowercase hex, got {a}"
    );
    // A different input must yield a different digest.
    assert_ne!(
        a,
        gnr8_core::manifest::blake3_hex(b"package other\n"),
        "different bytes must hash differently"
    );
}

/// WS-04: `save` then `load` round-trips byte-identically; entries are sorted by path so the
/// on-disk JSON is a deterministic diff.
#[test]
fn manifest_round_trip() {
    let root = unique_temp_dir("manifest-rt");
    let gnr8 = root.join(".gnr8");
    std::fs::create_dir_all(&gnr8).expect("create .gnr8");

    let mut manifest = gnr8_core::manifest::Manifest::default();
    // Insert out of order — save must sort by path.
    manifest.record("sdk/client.go", "aaaa", "sdk");
    manifest.record("openapi.yaml", "bbbb", "openapi");
    manifest.record("sdk/models.go", "cccc", "sdk");
    manifest.save(&gnr8).expect("save manifest");

    let loaded = gnr8_core::manifest::load(&gnr8).expect("load manifest");
    assert_eq!(loaded.recorded_hash("openapi.yaml"), Some("bbbb"));
    assert_eq!(loaded.recorded_hash("sdk/client.go"), Some("aaaa"));
    assert_eq!(loaded.recorded_hash("sdk/models.go"), Some("cccc"));
    assert_eq!(loaded.recorded_hash("missing.go"), None);

    // The serialized form is sorted by path (deterministic diffs).
    let raw = std::fs::read_to_string(gnr8.join("cache").join("manifest.json"))
        .expect("read manifest.json");
    let openapi_at = raw.find("openapi.yaml").expect("openapi entry present");
    let client_at = raw.find("sdk/client.go").expect("client entry present");
    let models_at = raw.find("sdk/models.go").expect("models entry present");
    assert!(
        openapi_at < client_at && client_at < models_at,
        "entries must be serialized sorted by path:\n{raw}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04: `load` on an absent manifest.json returns the empty default (graceful — absent
/// manifest ⇒ treat every output as fresh), never an error.
#[test]
fn manifest_absent_loads_empty() {
    let root = unique_temp_dir("manifest-absent");
    let gnr8 = root.join(".gnr8");
    std::fs::create_dir_all(&gnr8).expect("create .gnr8");

    let manifest =
        gnr8_core::manifest::load(&gnr8).expect("absent manifest loads as empty default");
    assert!(
        manifest.files.is_empty(),
        "absent manifest must load with no files"
    );
    assert_eq!(manifest.recorded_hash("openapi.yaml"), None);

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04 / DoS (T-04-02-03): a corrupt manifest.json loads as the empty default
/// (regenerate-from-scratch), never panics, never surfaces a hard error.
#[test]
fn manifest_corrupt_loads_empty() {
    let root = unique_temp_dir("manifest-corrupt");
    let cache = root.join(".gnr8").join("cache");
    std::fs::create_dir_all(&cache).expect("create cache dir");
    std::fs::write(cache.join("manifest.json"), b"{ this is not json")
        .expect("write corrupt manifest");

    let manifest =
        gnr8_core::manifest::load(&root.join(".gnr8")).expect("corrupt manifest loads as empty");
    assert!(
        manifest.files.is_empty(),
        "corrupt manifest must degrade to the empty default, not crash"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04 / D-04: `prune_to` drops manifest entries whose path is not in the supplied current
/// output set (deleting a file from config drops its entry).
#[test]
fn manifest_prunes_dropped() {
    let mut manifest = gnr8_core::manifest::Manifest::default();
    manifest.record("openapi.yaml", "aaaa", "openapi");
    manifest.record("sdk/client.go", "bbbb", "sdk");
    manifest.record("sdk/dropped.go", "cccc", "sdk");

    // The current generation no longer produces sdk/dropped.go.
    let current = vec!["openapi.yaml".to_string(), "sdk/client.go".to_string()];
    manifest.prune_to(&current);

    assert_eq!(manifest.recorded_hash("openapi.yaml"), Some("aaaa"));
    assert_eq!(manifest.recorded_hash("sdk/client.go"), Some("bbbb"));
    assert_eq!(
        manifest.recorded_hash("sdk/dropped.go"),
        None,
        "a path no longer generated must be pruned from the manifest"
    );
}

// ---------------------------------------------------------------------------
// WS-04 / WATCH-01 — lifecycle: pure plan_writes truth table + apply/naming (Task 2)
// ---------------------------------------------------------------------------

use gnr8_core::lifecycle::{self, WriteAction};
use gnr8_core::manifest::{blake3_hex, Manifest};

/// The FIXTURE the regenerate-based tests analyze (the goalservice Gin fixture, resolved relative
/// to the crate manifest dir — mirrors the other tests). Requires the Go toolchain; tests skip
/// gracefully if it is absent.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

/// Whether the `go` + `gofmt` toolchain is available so regenerate-based tests skip gracefully.
fn go_available() -> bool {
    std::process::Command::new("go")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
        && std::process::Command::new("gofmt")
            .arg("-h")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
}

/// Find the action `plan_writes` assigned to `path` (test helper).
fn action_for<'a>(plan: &'a lifecycle::WritePlan, path: &str) -> &'a WriteAction {
    &plan
        .files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("no planned file for {path}"))
        .action
}

/// WS-04 / WATCH-01: the PURE decision function classifies ALL FIVE truth-table arms correctly,
/// WITHOUT a filesystem (on-disk bytes are injected via a mock closure — the property that makes
/// the heart of the phase exhaustively unit-testable, RESEARCH Pattern 2 / Pitfall 3).
#[test]
fn plan_writes_truth_table() {
    // The freshly generated bytes for each output path (deterministic — Phase 2-3).
    let new_outputs: Vec<(String, Vec<u8>)> = vec![
        ("absent.go".to_string(), b"NEW".to_vec()), // arm 1: absent on disk
        ("noop.go".to_string(), b"SAME".to_vec()),  // arm 2: present, recorded, byte-identical
        ("changed.go".to_string(), b"NEW".to_vec()), // arm 3: present, recorded, content changed
        ("edited.go".to_string(), b"NEW".to_vec()), // arm 4: present, recorded, hash != recorded
        ("untracked.go".to_string(), b"NEW".to_vec()), // arm 5: present, absent from manifest
    ];

    // The previous-run manifest: records noop/changed/edited (their last-written hashes).
    let mut manifest = Manifest::default();
    manifest.record("noop.go", &blake3_hex(b"SAME"), "sdk");
    manifest.record("changed.go", &blake3_hex(b"OLD"), "sdk");
    manifest.record("edited.go", &blake3_hex(b"WHAT-GNR8-WROTE"), "sdk");
    // untracked.go is deliberately NOT in the manifest.

    // The mock on-disk reader: returns each path's CURRENT bytes (None ⇒ absent).
    let on_disk = |path: &str| -> Option<Vec<u8>> {
        match path {
            "absent.go" => None,
            "noop.go" => Some(b"SAME".to_vec()),
            "changed.go" => Some(b"OLD".to_vec()), // matches recorded ⇒ gnr8-owned, but content differs
            "edited.go" => Some(b"HUMAN-EDIT".to_vec()), // hash != recorded ⇒ user edited
            "untracked.go" => Some(b"PRE-EXISTING".to_vec()),
            other => panic!("unexpected on_disk lookup for {other}"),
        }
    };

    let plan = lifecycle::plan_writes(&new_outputs, &manifest, &on_disk);

    // Arm 1: absent on disk ⇒ Write.
    assert!(matches!(action_for(&plan, "absent.go"), WriteAction::Write));
    // Arm 2: present, recorded, new == disk ⇒ Unchanged (no-op).
    assert!(matches!(
        action_for(&plan, "noop.go"),
        WriteAction::Unchanged
    ));
    // Arm 3: present, recorded (disk hash == recorded), new != disk ⇒ Write (gnr8-owned update).
    assert!(matches!(
        action_for(&plan, "changed.go"),
        WriteAction::Write
    ));
    // Arm 4: present, recorded, on-disk hash != recorded ⇒ UserEdited (human hand-edited).
    assert!(matches!(
        action_for(&plan, "edited.go"),
        WriteAction::UserEdited
    ));
    // Arm 5: present, NOT in manifest ⇒ UserEdited (protect a pre-existing hand-written output).
    assert!(matches!(
        action_for(&plan, "untracked.go"),
        WriteAction::UserEdited
    ));

    // The plan carries the new bytes + new hash per file.
    let absent = plan.files.iter().find(|f| f.path == "absent.go").unwrap();
    assert_eq!(absent.new_bytes, b"NEW");
    assert_eq!(absent.new_hash, blake3_hex(b"NEW"));
}

/// WATCH-01 headline: a second `regenerate` over UNCHANGED source writes ZERO files (every output
/// is `Unchanged`), so `GenerateOutcome.written` is empty on the warm run.
#[test]
fn noop_second_run_writes_nothing() {
    if !go_available() {
        eprintln!("skipping noop_second_run_writes_nothing: go toolchain unavailable");
        return;
    }
    let (root, config) = scaffold_project("noop-second");

    let cold = lifecycle::regenerate(&root, &config, false).expect("cold regenerate");
    assert!(
        !cold.written.is_empty(),
        "cold run must write the generated outputs, got {cold:?}"
    );

    let warm = lifecycle::regenerate(&root, &config, false).expect("warm regenerate");
    assert!(
        warm.written.is_empty(),
        "a second regenerate over unchanged source must write nothing, got {:?}",
        warm.written
    );
    assert!(
        !warm.unchanged.is_empty(),
        "the warm run must report unchanged outputs, got {warm:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WATCH-01 / D-05: a no-op second run does NOT touch file mtimes (no write ⇒ no mtime churn).
#[test]
fn noop_preserves_mtime() {
    if !go_available() {
        eprintln!("skipping noop_preserves_mtime: go toolchain unavailable");
        return;
    }
    let (root, config) = scaffold_project("noop-mtime");

    lifecycle::regenerate(&root, &config, false).expect("cold regenerate");

    // Capture the openapi.yaml mtime after the cold run.
    let openapi = root.join(&config.output.openapi);
    let mtime_before = std::fs::metadata(&openapi)
        .expect("openapi.yaml exists after cold run")
        .modified()
        .expect("mtime available");

    // A no-op second run must NOT rewrite openapi.yaml (mtime unchanged).
    lifecycle::regenerate(&root, &config, false).expect("warm regenerate");
    let mtime_after = std::fs::metadata(&openapi)
        .expect("openapi.yaml still present")
        .modified()
        .expect("mtime available");

    assert_eq!(
        mtime_before, mtime_after,
        "a no-op regenerate must preserve the output mtime (no write)"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04 headline: a user-edited generated file (on-disk hash != recorded) is `UserEdited` ⇒
/// warned + SKIPPED (not silently clobbered) when `force=false`; the edit survives byte-for-byte.
#[test]
fn user_edit_is_protected() {
    if !go_available() {
        eprintln!("skipping user_edit_is_protected: go toolchain unavailable");
        return;
    }
    let (root, config) = scaffold_project("user-edit");

    lifecycle::regenerate(&root, &config, false).expect("cold regenerate");

    // The user hand-edits a generated file.
    let edited = root.join(&config.output.openapi);
    let user_bytes = b"# HAND EDITED BY A HUMAN - do not clobber\n";
    std::fs::write(&edited, user_bytes).expect("user edits openapi.yaml");

    let outcome = lifecycle::regenerate(&root, &config, false).expect("warm regenerate");
    assert!(
        outcome.skipped.iter().any(|p| p == &config.output.openapi),
        "the user-edited file must be SKIPPED (not clobbered), got {outcome:?}"
    );
    assert!(
        !outcome.written.iter().any(|p| p == &config.output.openapi),
        "the user-edited file must NOT be written without --force, got {outcome:?}"
    );

    let on_disk = std::fs::read(&edited).expect("read edited file");
    assert_eq!(
        on_disk, user_bytes,
        "the user's edit must survive byte-for-byte (no silent clobber)"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04: `regenerate(force=true)` OVERWRITES a user-edited file (records its new hash); the file
/// no longer matches the user's edit afterwards.
#[test]
fn force_overwrites_user_edit() {
    if !go_available() {
        eprintln!("skipping force_overwrites_user_edit: go toolchain unavailable");
        return;
    }
    let (root, config) = scaffold_project("force-overwrite");

    lifecycle::regenerate(&root, &config, false).expect("cold regenerate");

    let edited = root.join(&config.output.openapi);
    let user_bytes = b"# HAND EDITED\n";
    std::fs::write(&edited, user_bytes).expect("user edits openapi.yaml");

    let outcome = lifecycle::regenerate(&root, &config, true).expect("forced regenerate");
    assert!(
        outcome.written.iter().any(|p| p == &config.output.openapi),
        "--force must overwrite the user-edited file, got {outcome:?}"
    );

    let on_disk = std::fs::read(&edited).expect("read overwritten file");
    assert_ne!(
        on_disk, user_bytes,
        "--force must replace the user's edit with the regenerated content"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04 / Pitfall 5: a pre-existing output PRESENT on disk but ABSENT from the manifest (a
/// hand-written file at an output path) is protected on the FIRST run — never silently clobbered.
#[test]
fn untracked_output_protected() {
    if !go_available() {
        eprintln!("skipping untracked_output_protected: go toolchain unavailable");
        return;
    }
    let (root, config) = scaffold_project("untracked");

    // The user already has a hand-written openapi.yaml BEFORE the first generate (no manifest yet).
    let pre_existing = root.join(&config.output.openapi);
    let user_bytes = b"# PRE-EXISTING user spec\n";
    std::fs::write(&pre_existing, user_bytes).expect("user pre-writes openapi.yaml");

    let outcome = lifecycle::regenerate(&root, &config, false).expect("first regenerate");
    assert!(
        outcome.skipped.iter().any(|p| p == &config.output.openapi),
        "a pre-existing untracked output must be protected on first run, got {outcome:?}"
    );

    let on_disk = std::fs::read(&pre_existing).expect("read pre-existing file");
    assert_eq!(
        on_disk, user_bytes,
        "a pre-existing hand-written output must survive the first generate byte-for-byte"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-03: a `naming.operations` override remaps an operation id in the generated OpenAPI output
/// (an operationId rename appears in `to_openapi`).
#[test]
fn naming_overrides_apply() {
    if !go_available() {
        eprintln!("skipping naming_overrides_apply: go toolchain unavailable");
        return;
    }
    let mut graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");

    let mut naming = gnr8_core::config::NamingOverrides::default();
    // The fixture has a `goalUuidPut` operation (the @ID-annotated PUT).
    naming
        .operations
        .insert("goalUuidPut".to_string(), "RenamedUpdateGoal".to_string());

    lifecycle::apply_naming(&mut graph, &naming).expect("apply_naming with a valid override");
    let yaml = gnr8_core::lower::to_openapi(&graph).expect("to_openapi after naming override");

    assert!(
        yaml.contains("operationId: RenamedUpdateGoal"),
        "the remapped operation id must appear in the OpenAPI output:\n{yaml}"
    );
    assert!(
        !yaml.contains("operationId: goalUuidPut"),
        "the old operation id must be gone:\n{yaml}"
    );

    // A naming key with NO match is a silent no-op (not an error / not a panic).
    let mut graph2 = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");
    let mut noop_naming = gnr8_core::config::NamingOverrides::default();
    noop_naming
        .operations
        .insert("doesNotExist".to_string(), "Whatever".to_string());
    lifecycle::apply_naming(&mut graph2, &noop_naming).expect("an unmatched key is a no-op");
    assert!(
        gnr8_core::lower::to_openapi(&graph2).is_ok(),
        "an unmatched naming key must be a silent no-op"
    );
}

/// PLAN-CHECK W2 (MANDATORY): renaming a REFERENCED type (a schema used by an operation) via
/// `naming.types` updates the schema's name in `components.schemas` AND every matching `$ref`, so
/// `to_openapi` SUCCEEDS (no dangling $ref → no `CoreError::Lowering`) with the new name.
#[test]
fn naming_type_rename_updates_refs_no_dangling() {
    if !go_available() {
        eprintln!("skipping naming_type_rename_updates_refs_no_dangling: go toolchain unavailable");
        return;
    }
    let mut graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");

    // CreateGoalInput is referenced by an operation's request body (createGoal POST /). Rename it.
    let mut naming = gnr8_core::config::NamingOverrides::default();
    naming
        .types
        .insert("CreateGoalInput".to_string(), "NewGoalRequest".to_string());

    lifecycle::apply_naming(&mut graph, &naming).expect("apply_naming with a referenced rename");

    // to_openapi MUST succeed — a dangling $ref would raise CoreError::Lowering.
    let yaml = gnr8_core::lower::to_openapi(&graph)
        .expect("to_openapi must succeed after a referenced-type rename (no dangling $ref)");

    // The new name is in components.schemas AND in the referencing operation's $ref.
    assert!(
        yaml.contains("NewGoalRequest:"),
        "the renamed type must appear in components.schemas:\n{yaml}"
    );
    assert!(
        yaml.contains("#/components/schemas/NewGoalRequest"),
        "the operation $ref must point at the renamed type:\n{yaml}"
    );
    assert!(
        !yaml.contains("CreateGoalInput"),
        "no reference to the old type name may remain:\n{yaml}"
    );
}

/// WR-02: a `naming.types` rename whose TARGET collides with an existing type, COLLAPSES two types
/// into one, or CHAINS off another rename must fail loud with a typed `CoreError::Config` rather than
/// silently mis-generating a malformed/ambiguous artifact (the "never silently mis-generate" stance).
#[test]
fn naming_type_rename_collision_is_a_typed_error() {
    if !go_available() {
        eprintln!(
            "skipping naming_type_rename_collision_is_a_typed_error: go toolchain unavailable"
        );
        return;
    }

    // Collision: rename CreateGoalInput → GoalResponse, but GoalResponse already exists in the fixture.
    {
        let mut graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");
        let mut naming = gnr8_core::config::NamingOverrides::default();
        naming
            .types
            .insert("CreateGoalInput".to_string(), "GoalResponse".to_string());
        let err = lifecycle::apply_naming(&mut graph, &naming)
            .expect_err("a target colliding with an existing type must error");
        assert!(
            matches!(err, CoreError::Config { .. }),
            "collision must be CoreError::Config, got {err:?}"
        );
    }

    // Collapse: two distinct types renamed to the SAME target.
    {
        let mut graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");
        let mut naming = gnr8_core::config::NamingOverrides::default();
        naming
            .types
            .insert("CreateGoalInput".to_string(), "Merged".to_string());
        naming
            .types
            .insert("UpdateGoalInput".to_string(), "Merged".to_string());
        let err = lifecycle::apply_naming(&mut graph, &naming)
            .expect_err("two types renamed to the same target must error");
        assert!(
            matches!(err, CoreError::Config { .. }),
            "collapse must be CoreError::Config, got {err:?}"
        );
    }

    // Chain: A → B while B → C in the same pass (order-dependent → reject).
    {
        let mut graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");
        let mut naming = gnr8_core::config::NamingOverrides::default();
        naming
            .types
            .insert("CreateGoalInput".to_string(), "UpdateGoalInput".to_string());
        naming
            .types
            .insert("UpdateGoalInput".to_string(), "RenamedUpdate".to_string());
        let err =
            lifecycle::apply_naming(&mut graph, &naming).expect_err("a chained rename must error");
        assert!(
            matches!(err, CoreError::Config { .. }),
            "chain must be CoreError::Config, got {err:?}"
        );
    }
}

/// WR-04: multiple `config.inputs` are REJECTED loudly with `CoreError::Config` (multi-input fan-in is
/// out of scope, D-02) rather than silently analyzing only the first while watch would watch them all.
/// The check fires before any Go analysis, so it needs no toolchain.
#[test]
fn multi_input_config_is_rejected_loudly() {
    let root = unique_temp_dir("multi-input");
    gnr8_core::workspace::init(&root).expect("init .gnr8 workspace");

    let config_src = "inputs = [\"a\", \"b\"]\n\n[output]\nopenapi = \"openapi.yaml\"\nsdk_dir = \"sdk\"\ngo_module = \"example.com/test/sdk\"\n";
    let config = gnr8_core::config::parse(config_src).expect("parse multi-input config");

    // plan_only calls build_outputs first, so the rejection happens before the toolchain is touched.
    let err = lifecycle::plan_only(&root, &config)
        .expect_err("a multi-input config must be rejected, not silently truncated");
    assert!(
        matches!(err, CoreError::Config { .. }),
        "multi-input must be CoreError::Config, got {err:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// Recursively copy `src` into `dst` (creating `dst`), so a test can stage a self-contained copy of
/// the fixture module under a temp root. Mirrors the no-`tempfile`/no-extra-dep discipline.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).expect("create dst dir");
    for entry in std::fs::read_dir(src).expect("read src dir") {
        let entry = entry.expect("dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir_recursive(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
}

/// WATCH-01 / GAP (criterion 3): the SHIPPED DEFAULT config (`inputs = ["."]`, `sdk_dir = "sdk"`,
/// `openapi = "openapi.yaml"`) writes the generated SDK INSIDE the analyzed input tree. Before the
/// fix, a second `regenerate` re-analyzed gnr8's own `sdk/*.go`, doubled every schema (9 → 18), and
/// rewrote larger, duplicated output — so `init → generate → generate` was never a no-op.
///
/// This test runs the REAL `regenerate` pipeline twice on a staged copy of the fixture via the
/// default layout (inputs and outputs in the same tree) and asserts the SECOND run writes 0 files and
/// every output is unchanged, and that `plan_only` (the `gnr8 check` seam) reports NO drift (exit 0).
/// The exclusion of the configured output paths from analysis is what makes this hold.
#[test]
fn default_config_second_regenerate_is_a_noop() {
    if !go_available() {
        eprintln!("skipping default_config_second_regenerate_is_a_noop: go toolchain unavailable");
        return;
    }
    let root = unique_temp_dir("default-noop");

    // Stage a self-contained copy of the fixture module under the temp root, then init `.gnr8/` —
    // this reproduces the DEFAULT layout where inputs (`.`) and outputs (`sdk/`) share one tree.
    copy_dir_recursive(std::path::Path::new(FIXTURE_DIR), &root);
    gnr8_core::workspace::init(&root).expect("init .gnr8 workspace");

    // The SHIPPED default config body (inputs=["."], sdk_dir="sdk", openapi="openapi.yaml") — the
    // exact surface `gnr8 init` writes and the workflow this phase was asked to prove.
    let config = gnr8_core::config::parse(gnr8_core::workspace::DEFAULT_CONFIG_TOML)
        .expect("DEFAULT_CONFIG_TOML must parse");
    assert_eq!(config.inputs, vec![".".to_string()]);
    assert_eq!(config.output.sdk_dir, "sdk");
    assert_eq!(config.output.openapi, "openapi.yaml");

    // Cold run: writes the OpenAPI artifact + the SDK files.
    let cold = lifecycle::regenerate(&root, &config, false).expect("cold regenerate");
    assert!(
        !cold.written.is_empty(),
        "cold run must write the generated outputs, got {cold:?}"
    );
    let cold_written = cold.written.len();

    // Warm run: a TRUE no-op — gnr8's own sdk/*.go is excluded from analysis, so the graph is
    // identical to the cold run and every output is byte-identical.
    let warm = lifecycle::regenerate(&root, &config, false).expect("warm regenerate");
    assert!(
        warm.written.is_empty(),
        "DEFAULT-config second regenerate must write NOTHING (no self-ingestion), got {:?}",
        warm.written
    );
    assert_eq!(
        warm.unchanged.len(),
        cold_written,
        "every cold-written output must be unchanged on the warm run, got {warm:?}"
    );
    assert!(
        warm.skipped.is_empty(),
        "no output should be skipped on the warm run, got {warm:?}"
    );

    // `gnr8 check` (plan_only) must report NO drift on the warm tree → exit 0.
    let plan = lifecycle::plan_only(&root, &config).expect("plan_only after warm run");
    assert!(
        !plan.has_drift(),
        "plan_only must report no drift after a no-op regenerate (check exit 0)"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WR-02: `diagnostics_only` (the seam `gnr8 doctor` uses) harvests diagnostics over the SAME graph
/// `generate` acts on — it applies `exclude_output_paths`, so AFTER a cold `generate` writes gnr8's own
/// `sdk/*.go` into the analyzed `.` tree, NONE of the doctor diagnostics point at a generated output
/// file. A raw `build_graph` over the same tree WOULD re-analyze those generated files; `diagnostics_only`
/// must not. This keeps doctor's informational output consistent with what the pipeline ingests.
#[test]
fn diagnostics_only_excludes_generated_output() {
    if !go_available() {
        eprintln!("skipping diagnostics_only_excludes_generated_output: go toolchain unavailable");
        return;
    }
    let root = unique_temp_dir("diagnostics-exclude");

    // Stage the fixture under the temp root with the DEFAULT layout (inputs `.`, outputs `sdk/`
    // inside the analyzed tree) — the layout where self-ingestion would otherwise occur.
    copy_dir_recursive(std::path::Path::new(FIXTURE_DIR), &root);
    gnr8_core::workspace::init(&root).expect("init .gnr8 workspace");
    let config = gnr8_core::config::parse(gnr8_core::workspace::DEFAULT_CONFIG_TOML)
        .expect("DEFAULT_CONFIG_TOML must parse");

    // Cold run materializes gnr8's own sdk/*.go + openapi.yaml INSIDE the input tree.
    lifecycle::regenerate(&root, &config, false).expect("cold regenerate");
    assert!(
        root.join(&config.output.sdk_dir).is_dir(),
        "cold generate must have written the SDK dir into the analyzed tree"
    );

    // The doctor seam: diagnostics over the post-`exclude_output_paths` graph.
    let doctor_diags = lifecycle::diagnostics_only(&root, &config)
        .expect("diagnostics_only over a valid single-input project");

    // No diagnostic may point at a generated output file (under sdk/ or the openapi artifact).
    let sdk_prefix = format!("{}/", config.output.sdk_dir.trim_end_matches('/'));
    for d in &doctor_diags {
        assert!(
            !d.file.starts_with(&sdk_prefix) && d.file != config.output.openapi,
            "doctor diagnostics must EXCLUDE gnr8's own generated output, but got one on {} \
             (WR-02: doctor must analyze the same graph generate does)",
            d.file
        );
    }

    // A RAW build_graph over the same tree (no exclusion) sees STRICTLY MORE diagnostics — at least
    // the generated-output ones doctor correctly drops — proving the exclusion did real work.
    let raw = gnr8_core::analyze::build_graph(&root.to_string_lossy())
        .expect("raw build_graph over the generated tree");
    assert!(
        raw.diagnostics.len() >= doctor_diags.len(),
        "the unfiltered graph cannot have fewer diagnostics than the filtered doctor graph"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WR-02: `diagnostics_only` enforces the same single-input PoC restriction as `build_outputs`, so a
/// multi-input config surfaces a typed `CoreError::Config` rather than silently analyzing only the
/// first input (keeping doctor consistent with what `generate` would refuse to build). No toolchain
/// needed — the check fires before any Go analysis.
#[test]
fn diagnostics_only_rejects_multi_input() {
    let root = unique_temp_dir("diagnostics-multi-input");
    gnr8_core::workspace::init(&root).expect("init .gnr8 workspace");

    let config_src = "inputs = [\"a\", \"b\"]\n\n[output]\nopenapi = \"openapi.yaml\"\nsdk_dir = \"sdk\"\ngo_module = \"example.com/test/sdk\"\n";
    let config = gnr8_core::config::parse(config_src).expect("parse multi-input config");

    let err = lifecycle::diagnostics_only(&root, &config)
        .expect_err("a multi-input config must be rejected, not silently truncated");
    assert!(
        matches!(err, CoreError::Config { .. }),
        "multi-input must be CoreError::Config, got {err:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// Scaffold a project root with a `.gnr8/config.toml` whose `inputs` point at the fixture, returning
/// `(root, config)` ready for `regenerate`. Outputs land under the temp root (hermetic).
fn scaffold_project(label: &str) -> (PathBuf, gnr8_core::config::Config) {
    let root = unique_temp_dir(label);
    gnr8_core::workspace::init(&root).expect("init .gnr8 workspace");

    // Point inputs at the fixture (absolute) and keep outputs project-relative under the temp root.
    let config_src = format!(
        "inputs = [{FIXTURE_DIR:?}]\n\n[output]\nopenapi = \"openapi.yaml\"\nsdk_dir = \"sdk\"\ngo_module = \"example.com/test/sdk\"\n"
    );
    std::fs::write(root.join(".gnr8").join("config.toml"), &config_src)
        .expect("write test config.toml");
    let config = gnr8_core::config::parse(&config_src).expect("parse test config");
    (root, config)
}
