//! Lifecycle core tests: the mandatory `.gnr8/` crate scaffold (WS-01/WS-02), the blake3 ownership
//! manifest (WS-04), the PURE `plan_writes` truth table, and the host write machinery
//! (`regenerate`/`plan_only`) fed SYNTHETIC artifacts — plus the naming-override `$ref` rewrites.
//!
//! Config is now CODE: the host no longer extracts/lowers/generates in-process — the user's `.gnr8/`
//! child crate (the Pipeline) does. So these tests drive the host's WRITE half directly with synthetic
//! [`gnr8_core::sdk::Artifact`]s (no Go toolchain, no child process needed); the full host→child→write
//! path is exercised by the binary's `generate_e2e` integration test. The naming tests still drive
//! `apply_naming` + `lower::to_openapi` over the real fixture graph (they require the Go toolchain and
//! skip gracefully without it).
//!
//! Tests are hermetic — each creates a UNIQUE temp subdir under `std::env::temp_dir()`
//! (PID + nanosecond timestamp, no user-supplied path component; no `tempfile` crate).

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4 + ch.5); scope the
// allow to this test target so the workspace-wide RUST-04 deny stays intact for production code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown
)]

use std::path::PathBuf;

use gnr8_core::sdk::Artifact;
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

/// One synthetic artifact (a `(path, text)` pair) — what the child's pipeline would emit.
fn artifact(path: &str, text: &str) -> Artifact {
    Artifact {
        path: path.to_string(),
        text: text.to_string(),
    }
}

// ---------------------------------------------------------------------------
// WS-01 / WS-02 — workspace::init idempotent scaffold of the mandatory .gnr8/ crate
// ---------------------------------------------------------------------------

/// WS-01: `init` on a fresh dir creates `.gnr8/`, `.gnr8/src/`, and writes `Cargo.toml`,
/// `src/main.rs`, and `.gitignore`; `InitOutcome.created` lists those three files, `skipped` is empty.
#[test]
fn init_scaffolds_workspace() {
    let root = unique_temp_dir("scaffold");

    let outcome = gnr8_core::workspace::init(&root).expect("init on a fresh dir must succeed");

    let gnr8 = root.join(".gnr8");
    assert!(gnr8.is_dir(), ".gnr8/ must be created");
    assert!(gnr8.join("src").is_dir(), ".gnr8/src/ must be created");
    assert!(
        gnr8.join("Cargo.toml").is_file(),
        ".gnr8/Cargo.toml must be written"
    );
    assert!(
        gnr8.join("src").join("main.rs").is_file(),
        ".gnr8/src/main.rs must be written"
    );
    assert!(
        gnr8.join(".gitignore").is_file(),
        ".gnr8/.gitignore must be written"
    );

    for needle in ["Cargo.toml", "main.rs", ".gitignore"] {
        assert!(
            outcome.created.iter().any(|p| p.contains(needle)),
            "created must list {needle}, got {:?}",
            outcome.created
        );
    }
    assert!(
        outcome.skipped.is_empty(),
        "skipped must be empty on a fresh init, got {:?}",
        outcome.skipped
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-01: the scaffolded `Cargo.toml` is a standalone-workspace crate named `<dir>-gnr8-gen` with an
/// empty `[workspace]` table and a `gnr8-core` dependency; `src/main.rs` composes a `Pipeline`.
#[test]
fn scaffolded_crate_has_expected_shape() {
    let root = unique_temp_dir("shape");
    gnr8_core::workspace::init(&root).expect("init must succeed");

    let cargo = std::fs::read_to_string(root.join(".gnr8").join("Cargo.toml"))
        .expect("read .gnr8/Cargo.toml");
    assert!(
        cargo.contains("gnr8-gen"),
        "crate name must end in -gnr8-gen:\n{cargo}"
    );
    assert!(
        cargo.contains("[workspace]"),
        "must carry an empty [workspace] table (standalone crate):\n{cargo}"
    );
    assert!(
        cargo.contains("gnr8-core"),
        "must depend on gnr8-core:\n{cargo}"
    );
    assert!(
        cargo.contains("publish = false"),
        "must not be publishable:\n{cargo}"
    );

    let main_rs = std::fs::read_to_string(root.join(".gnr8").join("src").join("main.rs"))
        .expect("read .gnr8/src/main.rs");
    assert!(
        main_rs.contains("Pipeline::new()"),
        "main.rs must compose a Pipeline:\n{main_rs}"
    );
    assert!(
        main_rs.contains("gnr8_core::runner::run"),
        "main.rs must hand the pipeline to the runner:\n{main_rs}"
    );
    assert!(
        main_rs.contains("This file IS your gnr8 configuration"),
        "main.rs must carry the code-as-config doc comment:\n{main_rs}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-01 / D-01: re-running `init` after a user edits `src/main.rs` leaves the file byte-identical to
/// the edit (never clobbered); the second `InitOutcome.skipped` lists the files, `created` empty.
#[test]
fn init_is_idempotent() {
    let root = unique_temp_dir("idempotent");

    let first = gnr8_core::workspace::init(&root).expect("first init must succeed");
    assert!(!first.created.is_empty(), "first init must create files");

    // User edits src/main.rs — this content must survive a second init.
    let main_path = root.join(".gnr8").join("src").join("main.rs");
    let user_edit = b"// EDITED PIPELINE\nfn main() {}\n";
    std::fs::write(&main_path, user_edit).expect("user edits src/main.rs");

    let second = gnr8_core::workspace::init(&root).expect("second init must succeed");

    let on_disk = std::fs::read(&main_path).expect("read src/main.rs after second init");
    assert_eq!(
        on_disk, user_edit,
        "second init must preserve the user's src/main.rs edit byte-for-byte (D-01)"
    );

    assert!(
        second.created.is_empty(),
        "second init must create nothing, got {:?}",
        second.created
    );
    assert!(
        second.skipped.iter().any(|p| p.contains("main.rs")),
        "second init must skip src/main.rs, got {:?}",
        second.skipped
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-02: the written `.gnr8/.gitignore` body ignores the generation crate's build output + lifecycle
/// state (`/target/`, `/cache/`) while keeping `Cargo.toml`/`src/` checked in (it names neither).
#[test]
fn gitignore_splits_lifecycle() {
    let root = unique_temp_dir("gitignore");
    gnr8_core::workspace::init(&root).expect("init must succeed");

    let body = std::fs::read_to_string(root.join(".gnr8").join(".gitignore"))
        .expect("read .gnr8/.gitignore");

    assert!(
        body.contains("/target/") && body.contains("/cache/"),
        ".gitignore must ignore /target/ and /cache/, got:\n{body}"
    );
    assert!(
        !body.contains("Cargo.toml") && !body.contains("src"),
        ".gitignore must NOT ignore the checked-in crate files, got:\n{body}"
    );
    assert_eq!(
        body,
        gnr8_core::workspace::GITIGNORE_BODY,
        "written .gitignore must equal the GITIGNORE_BODY constant"
    );

    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// WS-04 — manifest: blake3-hashed ownership record
// ---------------------------------------------------------------------------

/// WS-04: `blake3_hex` is a stable 64-char lowercase hex digest — same input ⇒ same digest.
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
    assert_ne!(
        a,
        gnr8_core::manifest::blake3_hex(b"package other\n"),
        "different bytes must hash differently"
    );
}

/// WS-04: `save` then `load` round-trips byte-identically; entries are sorted by path.
#[test]
fn manifest_round_trip() {
    let root = unique_temp_dir("manifest-rt");
    let gnr8 = root.join(".gnr8");
    std::fs::create_dir_all(&gnr8).expect("create .gnr8");

    let mut manifest = gnr8_core::manifest::Manifest::default();
    manifest.record("sdk/client.go", "aaaa", "generated");
    manifest.record("openapi.yaml", "bbbb", "generated");
    manifest.record("sdk/models.go", "cccc", "generated");
    manifest.save(&gnr8).expect("save manifest");

    let loaded = gnr8_core::manifest::load(&gnr8).expect("load manifest");
    assert_eq!(loaded.recorded_hash("openapi.yaml"), Some("bbbb"));
    assert_eq!(loaded.recorded_hash("sdk/client.go"), Some("aaaa"));
    assert_eq!(loaded.recorded_hash("sdk/models.go"), Some("cccc"));
    assert_eq!(loaded.recorded_hash("missing.go"), None);

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

/// WS-04: `load` on an absent manifest.json returns the empty default (graceful), never an error.
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

/// WS-04 / DoS (T-04-02-03): a corrupt manifest.json loads as the empty default, never panics.
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

/// WS-04 / D-04: `prune_to` drops manifest entries whose path is not in the supplied current set.
#[test]
fn manifest_prunes_dropped() {
    let mut manifest = gnr8_core::manifest::Manifest::default();
    manifest.record("openapi.yaml", "aaaa", "generated");
    manifest.record("sdk/client.go", "bbbb", "generated");
    manifest.record("sdk/dropped.go", "cccc", "generated");

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
// WS-04 / WATCH-01 — lifecycle: PURE plan_writes truth table (synthetic Artifacts)
// ---------------------------------------------------------------------------

use gnr8_core::lifecycle::{self, WriteAction};
use gnr8_core::manifest::{blake3_hex, Manifest};

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
/// WITHOUT a filesystem (on-disk bytes are injected via a mock closure — the property that makes the
/// heart of the phase exhaustively unit-testable). Inputs are SYNTHETIC artifacts (no child needed).
#[test]
fn plan_writes_truth_table() {
    let artifacts = vec![
        artifact("absent.go", "NEW"),    // arm 1: absent on disk
        artifact("noop.go", "SAME"),     // arm 2: present, recorded, byte-identical
        artifact("changed.go", "NEW"),   // arm 3: present, recorded, content changed
        artifact("edited.go", "NEW"),    // arm 4: present, recorded, hash != recorded
        artifact("untracked.go", "NEW"), // arm 5: present, absent from manifest
    ];

    let mut manifest = Manifest::default();
    manifest.record("noop.go", &blake3_hex(b"SAME"), "generated");
    manifest.record("changed.go", &blake3_hex(b"OLD"), "generated");
    manifest.record("edited.go", &blake3_hex(b"WHAT-GNR8-WROTE"), "generated");
    // untracked.go is deliberately NOT in the manifest.

    let on_disk = |path: &str| -> Option<Vec<u8>> {
        match path {
            "absent.go" => None,
            "noop.go" => Some(b"SAME".to_vec()),
            "changed.go" => Some(b"OLD".to_vec()),
            "edited.go" => Some(b"HUMAN-EDIT".to_vec()),
            "untracked.go" => Some(b"PRE-EXISTING".to_vec()),
            other => panic!("unexpected on_disk lookup for {other}"),
        }
    };

    let plan = lifecycle::plan_writes(&artifacts, &manifest, &on_disk);

    assert!(matches!(action_for(&plan, "absent.go"), WriteAction::Write));
    assert!(matches!(
        action_for(&plan, "noop.go"),
        WriteAction::Unchanged
    ));
    assert!(matches!(
        action_for(&plan, "changed.go"),
        WriteAction::Write
    ));
    assert!(matches!(
        action_for(&plan, "edited.go"),
        WriteAction::UserEdited
    ));
    assert!(matches!(
        action_for(&plan, "untracked.go"),
        WriteAction::UserEdited
    ));

    let absent = plan.files.iter().find(|f| f.path == "absent.go").unwrap();
    assert_eq!(absent.new_bytes, b"NEW");
    assert_eq!(absent.new_hash, blake3_hex(b"NEW"));
}

// ---------------------------------------------------------------------------
// WS-04 / WATCH-01 — host write machinery: regenerate/plan_only over synthetic Artifacts
// ---------------------------------------------------------------------------

/// Init the `.gnr8/` crate under a fresh temp root so `regenerate`/`plan_only` find the manifest dir,
/// returning the root.
fn init_root(label: &str) -> PathBuf {
    let root = unique_temp_dir(label);
    gnr8_core::workspace::init(&root).expect("init .gnr8 workspace");
    root
}

/// WATCH-01 headline: a second `regenerate` over the SAME artifacts writes ZERO files (every output is
/// `Unchanged`). This is the pure host write half — synthetic artifacts, no child/Go needed.
#[test]
fn noop_second_run_writes_nothing() {
    let root = init_root("noop-second");
    let artifacts = vec![
        artifact("openapi.yaml", "openapi: 3.1.0\n"),
        artifact("sdk/client.go", "package sdk\n"),
    ];

    let cold = lifecycle::regenerate(&root, &artifacts, false).expect("cold regenerate");
    assert_eq!(
        cold.written.len(),
        2,
        "cold run must write both outputs, got {cold:?}"
    );

    let warm = lifecycle::regenerate(&root, &artifacts, false).expect("warm regenerate");
    assert!(
        warm.written.is_empty(),
        "a second regenerate over identical artifacts must write nothing, got {:?}",
        warm.written
    );
    assert_eq!(
        warm.unchanged.len(),
        2,
        "the warm run must report both outputs unchanged, got {warm:?}"
    );

    // plan_only (the `gnr8 check` seam) reports NO drift after the no-op.
    let plan = lifecycle::plan_only(&root, &artifacts).expect("plan_only after warm run");
    assert!(
        !plan.has_drift(),
        "plan_only must report no drift after a no-op regenerate (check exit 0)"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WATCH-01 / D-05: a no-op second run does NOT touch file mtimes (no write ⇒ no mtime churn).
#[test]
fn noop_preserves_mtime() {
    let root = init_root("noop-mtime");
    let artifacts = vec![artifact("openapi.yaml", "openapi: 3.1.0\n")];

    lifecycle::regenerate(&root, &artifacts, false).expect("cold regenerate");

    let openapi = root.join("openapi.yaml");
    let mtime_before = std::fs::metadata(&openapi)
        .expect("openapi.yaml exists after cold run")
        .modified()
        .expect("mtime available");

    lifecycle::regenerate(&root, &artifacts, false).expect("warm regenerate");
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

/// WS-04 headline: a user-edited generated file (on-disk hash != recorded) is `UserEdited` ⇒ SKIPPED
/// (not silently clobbered) when `force=false`; the edit survives byte-for-byte.
#[test]
fn user_edit_is_protected() {
    let root = init_root("user-edit");
    let artifacts = vec![artifact("openapi.yaml", "openapi: 3.1.0\n")];

    lifecycle::regenerate(&root, &artifacts, false).expect("cold regenerate");

    let edited = root.join("openapi.yaml");
    let user_bytes = b"# HAND EDITED BY A HUMAN - do not clobber\n";
    std::fs::write(&edited, user_bytes).expect("user edits openapi.yaml");

    let outcome = lifecycle::regenerate(&root, &artifacts, false).expect("warm regenerate");
    assert!(
        outcome.skipped.iter().any(|p| p == "openapi.yaml"),
        "the user-edited file must be SKIPPED (not clobbered), got {outcome:?}"
    );
    assert!(
        !outcome.written.iter().any(|p| p == "openapi.yaml"),
        "the user-edited file must NOT be written without --force, got {outcome:?}"
    );

    let on_disk = std::fs::read(&edited).expect("read edited file");
    assert_eq!(
        on_disk, user_bytes,
        "the user's edit must survive byte-for-byte (no silent clobber)"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04: `regenerate(force=true)` OVERWRITES a user-edited file (records its new hash).
#[test]
fn force_overwrites_user_edit() {
    let root = init_root("force-overwrite");
    let artifacts = vec![artifact("openapi.yaml", "openapi: 3.1.0\n")];

    lifecycle::regenerate(&root, &artifacts, false).expect("cold regenerate");

    let edited = root.join("openapi.yaml");
    let user_bytes = b"# HAND EDITED\n";
    std::fs::write(&edited, user_bytes).expect("user edits openapi.yaml");

    let outcome = lifecycle::regenerate(&root, &artifacts, true).expect("forced regenerate");
    assert!(
        outcome.written.iter().any(|p| p == "openapi.yaml"),
        "--force must overwrite the user-edited file, got {outcome:?}"
    );

    let on_disk = std::fs::read(&edited).expect("read overwritten file");
    assert_ne!(
        on_disk, user_bytes,
        "--force must replace the user's edit with the regenerated content"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// WS-04 / Pitfall 5: a pre-existing output PRESENT on disk but ABSENT from the manifest is protected
/// on the FIRST run — never silently clobbered.
#[test]
fn untracked_output_protected() {
    let root = init_root("untracked");
    let artifacts = vec![artifact("openapi.yaml", "openapi: 3.1.0\n")];

    let pre_existing = root.join("openapi.yaml");
    let user_bytes = b"# PRE-EXISTING user spec\n";
    std::fs::write(&pre_existing, user_bytes).expect("user pre-writes openapi.yaml");

    let outcome = lifecycle::regenerate(&root, &artifacts, false).expect("first regenerate");
    assert!(
        outcome.skipped.iter().any(|p| p == "openapi.yaml"),
        "a pre-existing untracked output must be protected on first run, got {outcome:?}"
    );

    let on_disk = std::fs::read(&pre_existing).expect("read pre-existing file");
    assert_eq!(
        on_disk, user_bytes,
        "a pre-existing hand-written output must survive the first generate byte-for-byte"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// T-04-02-01: an artifact whose path escapes the project root (`..`) is REJECTED with a typed
/// `CoreError::Io`, never written outside the root.
#[test]
fn regenerate_rejects_traversal_path() {
    let root = init_root("traversal");
    let artifacts = vec![artifact("../escape.go", "package x\n")];
    let err = lifecycle::regenerate(&root, &artifacts, false)
        .expect_err("a traversal output path must be rejected");
    assert!(
        matches!(err, CoreError::Io { .. }),
        "a traversal path must be CoreError::Io, got {err:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ---------------------------------------------------------------------------
// WS-03 — naming overrides via apply_naming + lower::to_openapi (real fixture graph)
// ---------------------------------------------------------------------------

/// The FIXTURE the naming tests analyze (the goalservice Gin fixture). Requires the Go toolchain;
/// tests skip gracefully if it is absent.
const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/goalservice");

/// Whether the `go` + `gofmt` toolchain is available so the naming tests skip gracefully.
fn go_available() -> bool {
    std::process::Command::new("go")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// The fixture's security schemes — the single source of truth for security (CLAUDE.md rule 4): one
/// `ApiKeyAuth` / `X-API-Key` scheme (graph-owned, as `ApplySecurity` would set them).
fn fixture_security() -> Vec<gnr8_core::graph::SecurityScheme> {
    vec![gnr8_core::graph::SecurityScheme {
        id: "ApiKeyAuth".to_string(),
        kind: "apiKey".to_string(),
        location: "header".to_string(),
        name: "X-API-Key".to_string(),
    }]
}

/// WS-03: a `naming.operations` override remaps an operation id in the generated OpenAPI output.
#[test]
fn naming_overrides_apply() {
    if !go_available() {
        eprintln!("skipping naming_overrides_apply: go toolchain unavailable");
        return;
    }
    let mut graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");

    let mut naming = lifecycle::NamingOverrides::default();
    naming
        .operations
        .insert("updateGoal".to_string(), "RenamedUpdateGoal".to_string());

    lifecycle::apply_naming(&mut graph, &naming).expect("apply_naming with a valid override");
    let yaml = gnr8_core::lower::to_openapi(&graph, "goalservice", "/goal", &fixture_security())
        .expect("to_openapi after naming override");

    assert!(
        yaml.contains("operationId: RenamedUpdateGoal"),
        "the remapped operation id must appear in the OpenAPI output:\n{yaml}"
    );
    assert!(
        !yaml.contains("operationId: updateGoal"),
        "the old operation id must be gone:\n{yaml}"
    );

    // A naming key with NO match is a silent no-op.
    let mut graph2 = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");
    let mut noop_naming = lifecycle::NamingOverrides::default();
    noop_naming
        .operations
        .insert("doesNotExist".to_string(), "Whatever".to_string());
    lifecycle::apply_naming(&mut graph2, &noop_naming).expect("an unmatched key is a no-op");
    assert!(
        gnr8_core::lower::to_openapi(&graph2, "goalservice", "/goal", &fixture_security()).is_ok(),
        "an unmatched naming key must be a silent no-op"
    );
}

/// PLAN-CHECK W2 (MANDATORY): renaming a REFERENCED type via `naming.types` updates the schema's name
/// in `components.schemas` AND every matching `$ref`, so `to_openapi` SUCCEEDS (no dangling $ref).
#[test]
fn naming_type_rename_updates_refs_no_dangling() {
    if !go_available() {
        eprintln!("skipping naming_type_rename_updates_refs_no_dangling: go toolchain unavailable");
        return;
    }
    let mut graph = gnr8_core::analyze::build_graph(FIXTURE_DIR).expect("build_graph");

    let mut naming = lifecycle::NamingOverrides::default();
    naming
        .types
        .insert("CreateGoalInput".to_string(), "NewGoalRequest".to_string());

    lifecycle::apply_naming(&mut graph, &naming).expect("apply_naming with a referenced rename");

    let yaml = gnr8_core::lower::to_openapi(&graph, "goalservice", "/goal", &fixture_security())
        .expect("to_openapi must succeed after a referenced-type rename (no dangling $ref)");

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

/// WR-02: a `naming.types` rename whose TARGET collides / collapses / chains must fail loud with a
/// typed `CoreError::Config` rather than silently mis-generating.
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
        let mut naming = lifecycle::NamingOverrides::default();
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
        let mut naming = lifecycle::NamingOverrides::default();
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
        let mut naming = lifecycle::NamingOverrides::default();
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
