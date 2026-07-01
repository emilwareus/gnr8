//! End-to-end regression for native Go/Gin contract extraction into Go and TypeScript SDKs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gnr8::sdk::prelude::*;

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/gin-contract-regression"
);

const TSC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tsextract/node_modules/typescript/bin/tsc"
);

fn go_available() -> bool {
    Command::new("go")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn ts_available() -> bool {
    let node_ok = Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok();
    node_ok && Path::new(TSC).exists()
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-gin-contract-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_pipeline() -> Option<gnr8::sdk::RunOutcome> {
    if !go_available() {
        eprintln!("skipping gin_contract_regression: go toolchain unavailable");
        return None;
    }
    let fixture = unique_temp_dir("fixture");
    copy_fixture(Path::new(FIXTURE_DIR), &fixture);
    Some(
        Pipeline::new()
            .source(GoGin::new().inputs(["."]))
            .target(TsSdk::new().module("@example/sdk").to("generated/ts"))
            .target(GoSdk::new().module("example.com/sdk").to("generated/go"))
            .run(&Cx::new(&fixture))
            .expect("gin contract pipeline must generate SDKs"),
    )
}

fn copy_fixture(src: &Path, dst: &Path) {
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("read fixture entry");
        let name = entry.file_name();
        if name == ".gnr8" {
            continue;
        }
        let source = entry.path();
        let target = dst.join(&name);
        if source.is_dir() {
            std::fs::create_dir_all(&target).expect("create fixture subdir");
            copy_fixture(&source, &target);
        } else {
            std::fs::copy(&source, &target).expect("copy fixture file");
        }
    }
}

fn artifact<'a>(outcome: &'a gnr8::sdk::RunOutcome, path: &str) -> &'a str {
    outcome
        .artifacts
        .files()
        .iter()
        .find(|artifact| artifact.path == path)
        .unwrap_or_else(|| panic!("missing artifact {path}"))
        .text
        .as_str()
}

#[test]
fn go_gin_contract_pipeline_generates_expected_sdk_surfaces() {
    let Some(outcome) = run_pipeline() else {
        return;
    };

    let ts_client = artifact(&outcome, "generated/ts/client.ts");
    assert!(ts_client.contains("method: \"PATCH\","), "{ts_client}");
    assert!(ts_client.contains("Promise<Blob>"), "{ts_client}");
    assert!(
        ts_client.contains("return await res.blob();"),
        "{ts_client}"
    );
    assert!(ts_client.contains("get auth(): AuthApi"), "{ts_client}");
    assert!(ts_client.contains("get files(): FilesApi"), "{ts_client}");
    assert!(ts_client.contains("get items(): ItemsApi"), "{ts_client}");
    assert!(
        ts_client.contains("encodeURIComponent(String(itemId))"),
        "{ts_client}"
    );
    assert!(
        ts_client.contains("encodeURIComponent(String(childId))"),
        "{ts_client}"
    );

    let ts_models = artifact(&outcome, "generated/ts/models.ts");
    assert!(
        ts_models.contains("export type ListSavedViews200Response = models.SavedViewResponse[];")
            || ts_models.contains("export type ListSavedViews200Response = SavedViewResponse[];"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("export interface CreateJob202Response"),
        "{ts_models}"
    );

    let go_ops = artifact(&outcome, "generated/go/operations.go");
    assert!(go_ops.contains("\"PATCH\""), "{go_ops}");
    assert!(go_ops.contains("[]byte"), "{go_ops}");
    assert!(go_ops.contains("io.ReadAll(resp.Body)"), "{go_ops}");
    assert!(go_ops.contains("type AuthAPI struct"), "{go_ops}");
    assert!(
        go_ops.contains("func (c *Client) Auth() *AuthAPI"),
        "{go_ops}"
    );
    assert!(go_ops.contains("type FilesAPI struct"), "{go_ops}");
    assert!(go_ops.contains("type ItemsAPI struct"), "{go_ops}");

    for file in outcome.artifacts.files() {
        assert!(
            !file.text.contains("gin.H") && !file.text.contains("github.com/gin-gonic/gin.H"),
            "{} must not contain gin.H refs",
            file.path
        );
    }
}

#[test]
fn generated_go_and_typescript_sdks_compile() {
    let Some(outcome) = run_pipeline() else {
        return;
    };

    let root = unique_temp_dir("compile");
    let go_dir = root.join("go");
    let ts_dir = root.join("ts");
    write_artifacts(&outcome, "generated/go/", &go_dir);
    write_artifacts(&outcome, "generated/ts/", &ts_dir);

    let go = Command::new("go")
        .args(["test", "./..."])
        .current_dir(&go_dir)
        .env("GOPROXY", "off")
        .output()
        .expect("spawn go test");
    assert!(
        go.status.success(),
        "generated Go SDK must compile:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&go.stdout),
        String::from_utf8_lossy(&go.stderr)
    );

    if !ts_available() {
        eprintln!("skipping TypeScript typecheck: node/tsc unavailable");
        return;
    }
    let ts = Command::new("node")
        .args([
            TSC,
            "--noEmit",
            "--strict",
            "--target",
            "es2022",
            "--module",
            "esnext",
            "--moduleResolution",
            "bundler",
            "--lib",
            "es2022,dom",
            "client.ts",
            "errors.ts",
            "index.ts",
            "models.ts",
        ])
        .current_dir(&ts_dir)
        .output()
        .expect("spawn tsc");
    assert!(
        ts.status.success(),
        "generated TypeScript SDK must typecheck:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ts.stdout),
        String::from_utf8_lossy(&ts.stderr)
    );
}

fn write_artifacts(outcome: &gnr8::sdk::RunOutcome, prefix: &str, dir: &Path) {
    for artifact in outcome.artifacts.files() {
        let Some(relative) = artifact.path.strip_prefix(prefix) else {
            continue;
        };
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create artifact dir");
        }
        std::fs::write(path, &artifact.text).expect("write artifact");
    }
}
