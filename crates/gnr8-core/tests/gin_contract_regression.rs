//! End-to-end regression for native Go/Gin contract extraction into Go and TypeScript SDKs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use gnr8_engine::sdk::prelude::*;

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/gin-contract-regression"
);

const TSC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tsextract/node_modules/typescript/bin/tsc"
);

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
        "gnr8-gin-contract-{label}-{}-{nanos}-{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_pipeline() -> Option<gnr8_engine::pipeline::PipelineOutcome> {
    if !go_available() {
        eprintln!("skipping gin_contract_regression: go toolchain unavailable");
        return None;
    }
    let fixture = unique_temp_dir("fixture");
    copy_fixture(Path::new(FIXTURE_DIR), &fixture);
    // A store of this test's own: the source analysis is shared through one, and a test must never
    // read from or write to the store the developer running it keeps for their own projects.
    let store = gnr8_engine::store::Store::at(fixture.join("cache-store"));
    let pipeline = Pipeline::new()
        .source(GoGin::new().inputs(["."]))
        .transform(ApiOverrides::new().sse_response("GET", "/v1/items/raw-stream"))
        .target(OpenApi31::new().to("generated/openapi.yaml"))
        .target(TsSdk::new().module("@example/sdk").to("generated/ts"))
        .target(PySdk::new().module("example_sdk").to("generated/py"))
        .target(
            PySdk::new()
                .module("example_wire")
                .dataclasses()
                .to("generated/py-wire"),
        )
        .target(GoSdk::new().module("example.com/sdk").to("generated/go"));
    Some(
        gnr8_engine::pipeline::run_in_process(&pipeline, &Cx::new(&fixture), Some(&store))
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

fn artifact<'a>(outcome: &'a gnr8_engine::pipeline::PipelineOutcome, path: &str) -> &'a str {
    outcome
        .artifacts
        .iter()
        .find(|artifact| artifact.path == path)
        .unwrap_or_else(|| panic!("missing artifact {path}"))
        .text
        .as_str()
}

fn assert_typescript_client(ts_client: &str) {
    assert!(
        ts_client.contains("headers[\"Content-Type\"] = \"application/json\";"),
        "{ts_client}"
    );
    assert!(
        ts_client.contains("const res = await this._request(")
            && ts_client.contains("\"PATCH\",")
            && ts_client.contains("body,")
            && ts_client.contains("operationId: \"updateItem\","),
        "{ts_client}"
    );
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
    assert!(
        ts_client.contains("export type UploadFileBody =")
            && ts_client.contains("contentType: \"application/json\"")
            && ts_client.contains("contentType: \"multipart/form-data\"")
            && ts_client.contains("files?: Array<Blob | ArrayBuffer | Uint8Array>")
            && ts_client
                .contains("redirect: options.followRedirects === true ? \"follow\" : \"manual\""),
        "{ts_client}"
    );
}

fn assert_typescript_models(ts_models: &str) {
    assert!(
        ts_models.contains("export type ListSavedViews200Response = models.SavedViewResponse[];")
            || ts_models.contains("export type ListSavedViews200Response = SavedViewResponse[];"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("export interface CreateJob202Response"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("  userUuid: string | null;"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("  items: ItemResponse[] | null;"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("  metadata: Record<string, string> | null;"),
        "{ts_models}"
    );
    assert!(ts_models.contains("  nickname?: string;"), "{ts_models}");
    assert!(ts_models.contains("  tags?: string[];"), "{ts_models}");
    assert!(
        ts_models.contains("  result?: Record<string, string>;"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("  zero?: Record<string, string>;"),
        "{ts_models}"
    );
    assert!(ts_models.contains("  ids: string[];"), "{ts_models}");
    assert!(
        ts_models.contains("  raw: Record<string, unknown> | null;"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("export interface SharedPayloadInput"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("export interface SharedPayloadOutput"),
        "{ts_models}"
    );
    assert!(
        ts_models.contains("  data?: string[] | null;"),
        "{ts_models}"
    );
    assert!(ts_models.contains("  data?: string[];"), "{ts_models}");
}

fn assert_python_models(py_models: &str) {
    assert!(
        py_models.contains("user_uuid: Optional[str] = Field(..., alias=\"userUuid\")"),
        "{py_models}"
    );
    assert!(py_models.contains("ids: list[str]"), "{py_models}");
    assert!(
        py_models.contains("raw: Optional[dict[str, Any]]"),
        "{py_models}"
    );
}

fn assert_openapi(openapi: &str) {
    let upload = path_section(openapi, "/v1/files/upload");
    assert!(
        upload.contains("required: true")
            && upload.contains("application/json:")
            && upload.contains("#/components/schemas/UpdateItemRequest")
            && upload.contains("multipart/form-data:")
            && upload.contains("#/components/schemas/UploadFileFormRequest"),
        "{upload}"
    );
    let upload_form = openapi
        .split("    UploadFileFormRequest:\n")
        .nth(1)
        .expect("UploadFileFormRequest component");
    assert!(
        upload_form.contains("files:")
            && upload_form.contains("format: binary")
            && upload_form.contains("request:\n          type: string")
            && !upload_form.contains("required:"),
        "{upload_form}"
    );

    let redirect = path_section(openapi, "/v1/files/{fileId}/redirect");
    assert!(
        redirect.contains("'307':")
            && redirect.contains("Location:")
            && redirect.contains("X-Session-ID:"),
        "{redirect}"
    );
    let helper_redirect = path_section(openapi, "/v1/files/{fileId}/helper-redirect");
    assert!(
        helper_redirect.contains("'302':") && helper_redirect.contains("Location:"),
        "{helper_redirect}"
    );
    let read = path_section(openapi, "/v1/files/{fileId}/read");
    for header in [
        "Content-Disposition:",
        "Content-Length:",
        "Content-Type:",
        "X-Session-ID:",
    ] {
        assert!(read.contains(header), "missing {header} in {read}");
    }

    let observations = path_section(openapi, "/v1/items/request-observations");
    assert!(
        observations.contains("name: X-Observed\n        in: header\n        required: false")
            && observations
                .contains("name: X-Required\n        in: header\n        required: true")
            && observations
                .contains("name: observed-cookie\n        in: cookie\n        required: false")
            && observations
                .contains("name: required-cookie\n        in: cookie\n        required: true")
            && !observations.contains("Authorization"),
        "{observations}"
    );
    let search = path_section(openapi, "/v1/items/search");
    assert!(
        search.contains("name: offset\n        in: query\n        required: false")
            && search.contains("name: page\n        in: query\n        required: true")
            && search.contains("default: first")
            && search.contains("default: asc"),
        "{search}"
    );

    let directional = openapi
        .split("    DirectionalResponse:\n")
        .nth(1)
        .expect("DirectionalResponse component");
    let directional = directional
        .split("    ItemResponse:\n")
        .next()
        .unwrap_or(directional);
    assert!(
        directional.contains("required: [items, metadata, userUuid]"),
        "{directional}"
    );
    let result = directional
        .split("        result:\n")
        .nth(1)
        .expect("result property")
        .split("        userUuid:\n")
        .next()
        .expect("bounded result property");
    assert!(!result.contains("null"), "{result}");
    assert!(
        directional.contains(
            "        zero:\n          type: object\n          additionalProperties:\n            type: string"
        ),
        "{directional}"
    );

    let validated = openapi
        .split("    ValidatedRequest:\n")
        .nth(1)
        .expect("ValidatedRequest component");
    assert!(validated.contains("required: [ids, raw]"), "{validated}");
    let ids = validated
        .split("        ids:\n")
        .nth(1)
        .expect("ids property")
        .split("        raw:\n")
        .next()
        .expect("bounded ids property");
    assert!(!ids.contains("null"), "{ids}");
}

fn path_section<'a>(openapi: &'a str, path: &str) -> &'a str {
    let marker = format!("  '{path}':\n");
    let section = openapi
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("missing OpenAPI path {path}"));
    section.split("\n  '").next().unwrap_or(section)
}

fn assert_go_operations(go_ops: &str) {
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
    assert!(go_ops.contains("type UploadFileBody interface"), "{go_ops}");
    assert!(
        go_ops.contains("type UploadFileJSONBody struct"),
        "{go_ops}"
    );
    assert!(
        go_ops.contains("type UploadFileMultipartBody struct"),
        "{go_ops}"
    );
    assert!(
        go_ops.contains("SuccessStatuses: map[int]bool{") && go_ops.contains("307: true,"),
        "{go_ops}"
    );
}

#[test]
fn go_gin_contract_pipeline_generates_expected_sdk_surfaces() {
    let Some(outcome) = run_pipeline() else {
        return;
    };

    assert_typescript_client(artifact(&outcome, "generated/ts/client.ts"));
    assert_typescript_models(artifact(&outcome, "generated/ts/models.ts"));
    assert_python_models(artifact(&outcome, "generated/py/models.py"));
    assert_openapi(artifact(&outcome, "generated/openapi.yaml"));
    assert_go_operations(artifact(&outcome, "generated/go/operations.go"));

    assert!(
        outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "security.requirement.missing"
                && diagnostic.operation.as_deref() == Some("GET /v1/items/request-observations")
        }),
        "Authorization reads must remain actionable until user code configures security: {:?}",
        outcome.diagnostics
    );

    for file in &outcome.artifacts {
        assert!(
            !file.text.contains("gin.H") && !file.text.contains("github.com/gin-gonic/gin.H"),
            "{} must not contain gin.H refs",
            file.path
        );
    }
}

#[test]
fn generated_sdks_compile() {
    let Some(outcome) = run_pipeline() else {
        return;
    };

    let root = unique_temp_dir("compile");
    let go_dir = root.join("go");
    let ts_dir = root.join("ts");
    let py_dir = root.join("py-wire");
    std::fs::write(
        root.join("openapi.yaml"),
        artifact(&outcome, "generated/openapi.yaml"),
    )
    .expect("write generated OpenAPI");
    write_artifacts(&outcome, "generated/go/", &go_dir);
    write_artifacts(&outcome, "generated/ts/", &ts_dir);
    write_artifacts(&outcome, "generated/py-wire/", &py_dir);

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

    let python = Command::new("python3")
        .args(["-m", "compileall", "-q", "."])
        .current_dir(&py_dir)
        .output()
        .expect("spawn python compileall");
    assert!(
        python.status.success(),
        "generated Python SDK must compile:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&python.stdout),
        String::from_utf8_lossy(&python.stderr)
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

#[test]
fn generated_sdks_encode_body_variants_and_redirects_with_fake_transports() {
    let Some(outcome) = run_pipeline() else {
        return;
    };
    let root = unique_temp_dir("wire");

    let go_dir = root.join("go");
    write_artifacts(&outcome, "generated/go/", &go_dir);
    std::fs::write(
        go_dir.join("gin_contract_wire_test.go"),
        include_str!("drivers/gin_contract/go_wire_test.go"),
    )
    .expect("write Go wire test");
    let go = Command::new("go")
        .args(["test", "./..."])
        .current_dir(&go_dir)
        .env("GOPROXY", "off")
        .output()
        .expect("spawn generated Go wire test");
    assert_command_success("generated Go fake transport", &go);

    let py_root = root.join("py");
    let py_package = py_root.join("example_wire");
    write_artifacts(&outcome, "generated/py-wire/", &py_package);
    let py_driver = py_root.join("py_wire_driver.py");
    std::fs::write(
        &py_driver,
        include_str!("drivers/gin_contract/py_wire_driver.py"),
    )
    .expect("write Python wire driver");
    let python = Command::new("python3")
        .arg(&py_driver)
        .current_dir(&py_root)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1")
        .output()
        .expect("spawn generated Python wire driver");
    assert_command_success("generated Python fake transport", &python);

    if !ts_available() {
        eprintln!("skipping TypeScript fake transport: node/tsc unavailable");
        return;
    }
    let ts_dir = root.join("ts");
    write_artifacts(&outcome, "generated/ts/", &ts_dir);
    std::fs::write(
        ts_dir.join("ts_wire_driver.ts"),
        include_str!("drivers/gin_contract/ts_wire_driver.ts"),
    )
    .expect("write TypeScript wire driver");
    let typescript = Command::new("node")
        .args([
            TSC,
            "--strict",
            "--target",
            "es2022",
            "--module",
            "commonjs",
            "--moduleResolution",
            "node",
            "--lib",
            "es2022,dom",
            "--outDir",
            "dist",
            "client.ts",
            "errors.ts",
            "index.ts",
            "models.ts",
            "ts_wire_driver.ts",
        ])
        .current_dir(&ts_dir)
        .output()
        .expect("spawn TypeScript compiler");
    assert_command_success("generated TypeScript fake transport typecheck", &typescript);
    let node = Command::new("node")
        .arg("dist/ts_wire_driver.js")
        .current_dir(&ts_dir)
        .output()
        .expect("spawn generated TypeScript wire driver");
    assert_command_success("generated TypeScript fake transport", &node);
}

fn assert_command_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_artifacts(outcome: &gnr8_engine::pipeline::PipelineOutcome, prefix: &str, dir: &Path) {
    for artifact in &outcome.artifacts {
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
