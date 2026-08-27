//! The host↔worker contract, end to end through the real `gnr8` binary.
//!
//! `generate_e2e` proves the whole orchestration over a Go source. This file proves the parts of the
//! contract that have nothing to do with a language toolchain, so it needs only `cargo`:
//!
//! - a `.gnr8/` manifest declaring the previous contract is refused **before** anything is compiled;
//! - `cargo` is invoked exactly once, and never again while `.gnr8/` is unchanged;
//! - each row of the invalidation matrix rebuilds exactly what it should;
//! - a user's own `Transform` and `Target` round-trip through the frame protocol;
//! - `--no-build` and `--no-execute` withhold consent, and say why.
//!
//! Every pipeline here uses the `OpenApi` source, which reads a YAML file the test writes, so no Go,
//! Python or Node toolchain is involved.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The installed `gnr8` host binary cargo built for this integration test.
const GNR8_BIN: &str = env!("CARGO_BIN_EXE_gnr8");

/// The in-repo thin SDK a scaffolded worker depends on.
fn sdk_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../gnr8-sdk")
        .canonicalize()
        .expect("the gnr8-sdk crate must exist beside the CLI crate")
}

fn cargo_available() -> bool {
    Command::new("cargo")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn unique_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "worker-contract-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const OPENAPI_SOURCE: &str = r"openapi: 3.0.3
info:
  title: Fixture
  version: 1.0.0
paths:
  /things:
    get:
      operationId: listThings
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                type: array
                items:
                  type: string
";

/// Write a project whose worker pipeline is `pipeline_body`, with the given extra items.
fn write_project(root: &Path, items: &str, pipeline_body: &str) {
    std::fs::create_dir_all(root.join(".gnr8/src")).unwrap();
    std::fs::write(root.join("openapi.yaml"), OPENAPI_SOURCE).unwrap();
    std::fs::write(
        root.join(".gnr8/Cargo.toml"),
        format!(
            "[package]\nname = \"contract-gnr8-gen\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             publish = false\n\n[dependencies]\ngnr8 = {{ path = {:?} }}\n\n[workspace]\n",
            sdk_path().to_string_lossy()
        ),
    )
    .unwrap();
    std::fs::write(
        root.join(".gnr8/src/main.rs"),
        format!(
            "use gnr8::sdk::prelude::*;\n\
             #[allow(unused_imports)]\n\
             use gnr8::graph::ApiGraph;\n\
             #[allow(unused_imports)]\n\
             use gnr8::Error;\n\n\
             {items}\n\n\
             fn main() -> std::process::ExitCode {{\n    \
             gnr8::worker::run(\n        Pipeline::new()\n{pipeline_body}    )\n}}\n"
        ),
    )
    .unwrap();
}

/// A sentinel `cargo` shim that appends one line per invocation, then delegates to the real cargo.
#[cfg(unix)]
fn install_cargo_sentinel(dir: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let log = dir.join("cargo-invocations.log");
    let shim = dir.join("cargo-sentinel.sh");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {:?}\nexec cargo \"$@\"\n",
            log.to_string_lossy()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    (shim, log)
}

#[cfg(unix)]
fn cargo_invocations(log: &Path) -> usize {
    std::fs::read_to_string(log).map_or(0, |text| {
        text.lines().filter(|line| !line.trim().is_empty()).count()
    })
}

fn gnr8(root: &Path, args: &[&str], cargo: Option<&Path>) -> Output {
    let mut command = Command::new(GNR8_BIN);
    command.args(args).current_dir(root);
    if let Some(cargo) = cargo {
        command.env("GNR8_CARGO", cargo);
    }
    command.output().expect("the gnr8 host must run")
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_manifest_declaring_the_previous_contract_is_refused_before_anything_is_compiled() {
    let root = unique_dir("old-contract");
    write_project(
        &root,
        "",
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n",
    );
    // The manifest, not the source, is what carries the contract.
    std::fs::write(
        root.join(".gnr8/Cargo.toml"),
        "[package]\nname = \"old-gnr8-gen\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\ngnr8 = \"=0.8.0\"\n\n[workspace]\n",
    )
    .unwrap();

    let output = gnr8(&root, &["check"], None);
    let text = combined(&output);

    assert!(!output.status.success(), "{text}");
    assert!(text.contains("previous .gnr8 contract"), "{text}");
    assert!(text.contains("gnr8 init --upgrade"), "{text}");
    assert!(
        !root.join(".gnr8/target").exists(),
        "nothing may be compiled before the manifest is accepted"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_manifest_depending_on_the_host_engine_is_refused() {
    let root = unique_dir("engine-dep");
    std::fs::create_dir_all(root.join(".gnr8/src")).unwrap();
    std::fs::write(
        root.join(".gnr8/Cargo.toml"),
        "[package]\nname = \"engine-gnr8-gen\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\ngnr8 = \"=0.9.0\"\ngnr8-engine = \"0.9.0\"\n\n[workspace]\n",
    )
    .unwrap();

    let text = combined(&gnr8(&root, &["check"], None));

    assert!(text.contains("host engine"), "{text}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn init_upgrade_repoints_the_manifest_and_leaves_the_users_rust_alone() {
    let root = unique_dir("upgrade");
    std::fs::create_dir_all(root.join(".gnr8/src")).unwrap();
    std::fs::write(
        root.join(".gnr8/Cargo.toml"),
        "# keep me\n[package]\nname = \"upgrade-gnr8-gen\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\ngnr8 = \"=0.8.0\"\nrand = \"0.8\"\n\n[workspace]\n",
    )
    .unwrap();
    let user_rust = "fn main() { /* mine */ }\n";
    std::fs::write(root.join(".gnr8/src/main.rs"), user_rust).unwrap();
    std::fs::write(root.join(".gnr8/Cargo.lock"), "stale\n").unwrap();

    let text = combined(&gnr8(&root, &["init", "--upgrade"], None));

    let manifest = std::fs::read_to_string(root.join(".gnr8/Cargo.toml")).unwrap();
    assert!(manifest.contains("# keep me"), "{manifest}");
    assert!(manifest.contains("rand = \"0.8\""), "{manifest}");
    assert!(!manifest.contains("=0.8.0"), "{manifest}");
    assert!(
        !root.join(".gnr8/Cargo.lock").exists(),
        "the lockfile pinned the previous dependency tree"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".gnr8/src/main.rs")).unwrap(),
        user_rust,
        "--upgrade must never rewrite the user's Rust"
    );
    assert!(text.contains("gnr8::worker::run("), "{text}");
    assert!(text.contains("Custom(...)"), "{text}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn no_execute_refuses_without_building_or_running_anything() {
    let root = unique_dir("no-execute");
    write_project(
        &root,
        "",
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n\
                     .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n",
    );

    let output = gnr8(&root, &["--no-execute", "check"], None);
    let text = combined(&output);

    assert!(!output.status.success(), "{text}");
    assert!(text.contains("--no-execute"), "{text}");
    assert!(
        !root.join(".gnr8/target").exists(),
        "--no-execute must not compile the worker"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn no_build_refuses_when_no_matching_worker_exists() {
    let root = unique_dir("no-build");
    write_project(
        &root,
        "",
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n\
                     .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n",
    );

    let output = gnr8(&root, &["--no-build", "check"], None);
    let text = combined(&output);

    assert!(!output.status.success(), "{text}");
    assert!(text.contains("--no-build"), "{text}");
    assert!(
        !root.join(".gnr8/target").exists(),
        "--no-build must not invoke cargo"
    );
    let _ = std::fs::remove_dir_all(root);
}

/// The whole invalidation matrix in one project, because each row costs a worker build otherwise.
#[cfg(unix)]
#[test]
fn cargo_runs_once_and_only_when_the_worker_inputs_change() {
    if !cargo_available() {
        eprintln!("skipping worker_contract: cargo unavailable");
        return;
    }
    let root = unique_dir("fingerprint");
    write_project(
        &root,
        "",
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n\
                     .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n",
    );
    let (cargo, log) = install_cargo_sentinel(&root);

    // 1. Cold: the worker does not exist, so cargo builds it.
    let first = gnr8(&root, &["generate"], Some(&cargo));
    assert!(first.status.success(), "{}", combined(&first));
    assert!(
        root.join("generated/openapi.yaml").is_file(),
        "the OpenAPI artifact must land on disk"
    );
    let built = cargo_invocations(&log);
    assert!(built >= 1, "the cold run must build the worker");

    // 2. Unchanged: the recorded binary still matches its fingerprint, so cargo is not invoked.
    let second = gnr8(&root, &["generate"], Some(&cargo));
    assert!(second.status.success(), "{}", combined(&second));
    assert_eq!(
        cargo_invocations(&log),
        built,
        "an unchanged project must not invoke cargo"
    );
    let third = gnr8(&root, &["check"], Some(&cargo));
    assert!(third.status.success(), "{}", combined(&third));
    assert_eq!(
        cargo_invocations(&log),
        built,
        "an unchanged `check` must not invoke cargo either"
    );

    // 3. A source-file change regenerates but does not rebuild the worker.
    std::fs::write(
        root.join("openapi.yaml"),
        OPENAPI_SOURCE.replace("title: Fixture", "title: Renamed"),
    )
    .unwrap();
    let fourth = gnr8(&root, &["generate"], Some(&cargo));
    assert!(fourth.status.success(), "{}", combined(&fourth));
    assert_eq!(
        cargo_invocations(&log),
        built,
        "a project source change must not rebuild the worker"
    );
    assert!(
        std::fs::read_to_string(root.join("generated/openapi.yaml"))
            .unwrap()
            .contains("Renamed"),
        "the source change must reach the artifact"
    );

    // 4. Touching the pipeline without changing its bytes is not a change.
    let main_rs = root.join(".gnr8/src/main.rs");
    let body = std::fs::read_to_string(&main_rs).unwrap();
    std::fs::write(&main_rs, &body).unwrap();
    let fifth = gnr8(&root, &["generate"], Some(&cargo));
    assert!(fifth.status.success(), "{}", combined(&fifth));
    assert_eq!(
        cargo_invocations(&log),
        built,
        "the fingerprint is content-addressed, so a rewritten-identical file is not a change"
    );

    // 5. A real pipeline edit rebuilds.
    std::fs::write(&main_rs, format!("{body}\n// a real edit\n")).unwrap();
    let sixth = gnr8(&root, &["generate"], Some(&cargo));
    assert!(sixth.status.success(), "{}", combined(&sixth));
    let after_edit = cargo_invocations(&log);
    assert!(
        after_edit > built,
        "a pipeline edit must rebuild the worker ({built} -> {after_edit})"
    );

    // 6. A tampered worker binary is rebuilt rather than trusted.
    let binary = root.join(".gnr8/target/debug/contract-gnr8-gen");
    let mut bytes = std::fs::read(&binary).unwrap();
    bytes.extend_from_slice(b"tamper");
    std::fs::write(&binary, bytes).unwrap();
    let seventh = gnr8(&root, &["generate"], Some(&cargo));
    assert!(seventh.status.success(), "{}", combined(&seventh));
    assert!(
        cargo_invocations(&log) > after_edit,
        "a worker binary that no longer hashes to its stamp must be rebuilt"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_users_own_transform_and_target_round_trip_through_the_worker() {
    if !cargo_available() {
        eprintln!("skipping worker_contract: cargo unavailable");
        return;
    }
    let root = unique_dir("custom");
    write_project(
        &root,
        r##"struct Retitle;
impl Transform for Retitle {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), Error> {
        ir.title = format!("{} (worker)", ir.title);
        Ok(())
    }
}

struct Summary;
impl Target for Summary {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), Error> {
        out.create("generated/SUMMARY.md", format!("# {}\n{} operations\n", ir.title, ir.operations.len()))
    }
    fn output_anchors(&self) -> Vec<String> {
        vec!["generated/SUMMARY.md".to_string()]
    }
}

struct Banner;
impl PostProcess for Banner {
    fn run(&self, out: &mut Artifacts, _cx: &Cx) -> Result<(), Error> {
        out.rewrite("generated/SUMMARY.md", |text| format!("<!-- generated -->\n{text}"))
    }
}
"##,
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n\
                     .transform(Custom(Retitle))\n\
                     .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n\
                     .target(Custom(Summary))\n\
                     .post(Custom(Banner))\n",
    );

    let output = gnr8(&root, &["generate", "-v"], None);
    let text = combined(&output);
    assert!(output.status.success(), "{text}");

    let summary = std::fs::read_to_string(root.join("generated/SUMMARY.md")).unwrap();
    assert_eq!(
        summary,
        "<!-- generated -->\n# Fixture (worker)\n1 operations\n"
    );

    // The built-in OpenAPI target ran host-side over the graph the worker transform mutated.
    let document = std::fs::read_to_string(root.join("generated/openapi.yaml")).unwrap();
    assert!(document.contains("Fixture (worker)"), "{document}");

    // A second run is a true no-op.
    let again = gnr8(&root, &["generate"], None);
    let again_text = combined(&again);
    assert!(again.status.success(), "{again_text}");
    assert!(again_text.contains("0 written"), "{again_text}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failing_user_stage_surfaces_its_own_message() {
    if !cargo_available() {
        eprintln!("skipping worker_contract: cargo unavailable");
        return;
    }
    let root = unique_dir("failing");
    write_project(
        &root,
        r#"struct Boom;
impl Transform for Boom {
    fn apply(&self, _ir: &mut ApiGraph, _cx: &Cx) -> Result<(), Error> {
        Err(Error::config("this pipeline refuses to run"))
    }
}
"#,
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n\
                     .transform(Custom(Boom))\n\
                     .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n",
    );

    let output = gnr8(&root, &["generate"], None);
    let text = combined(&output);

    assert!(!output.status.success(), "{text}");
    assert!(text.contains("this pipeline refuses to run"), "{text}");
    assert!(
        !root.join("generated/openapi.yaml").exists(),
        "a failed pipeline must write nothing"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn a_worker_binary_reached_through_a_symlinked_target_dir_is_refused() {
    if !cargo_available() {
        eprintln!("skipping worker_contract: cargo unavailable");
        return;
    }
    let root = unique_dir("symlinked-target");
    write_project(
        &root,
        "",
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n\
                     .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n",
    );
    assert!(gnr8(&root, &["generate"], None).status.success());

    // `target/` is excluded from the build fingerprint, so redirecting it must be caught by the
    // containment check rather than by an input hash.
    let real = root.join("elsewhere");
    std::fs::rename(root.join(".gnr8/target"), &real).unwrap();
    std::os::unix::fs::symlink(&real, root.join(".gnr8/target")).unwrap();

    let output = gnr8(&root, &["--no-build", "generate"], None);
    let text = combined(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("refuses to run it"), "{text}");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_worker_run_by_hand_reports_that_it_is_not_a_standalone_program() {
    if !cargo_available() {
        eprintln!("skipping worker_contract: cargo unavailable");
        return;
    }
    let root = unique_dir("by-hand");
    write_project(
        &root,
        "",
        "            .source(OpenApi::new().input(\"openapi.yaml\"))\n\
                     .target(OpenApi31::new().to(\"generated/openapi.yaml\"))\n",
    );
    assert!(gnr8(&root, &["generate"], None).status.success());

    let binary = root.join(".gnr8/target/debug/contract-gnr8-gen");
    let output = Command::new(&binary)
        .current_dir(&root)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the worker binary must be runnable");

    assert_eq!(output.status.code(), Some(2), "{}", combined(&output));
    assert!(
        combined(&output).contains("started by the `gnr8` host"),
        "{}",
        combined(&output)
    );
    let _ = std::fs::remove_dir_all(root);
}
