//! TSSDK-02 hermetic typecheck gate: the generated TypeScript SDK genuinely type-checks under the
//! VENDORED `typescript` compiler (`tsc --noEmit --strict`) — the load-bearing analog of the
//! `pysdk_compile` `py_compile`+import gate that caught real codegen bugs a string snapshot can't (a
//! bundle can look correct yet not type-check, RESEARCH Pitfall 3). The TypeScript twin of
//! `tests/pysdk_compile.rs`, MINUS the round-trip http.server driver (TSSDK-02 asks only for
//! `tsc --noEmit`, RESEARCH Open Q3).
//!
//! The harness (1) builds the graph from the `nestjs-bookstore` fixture via the Phase-4 `tsextract`
//! path that `build_graph` routes to (needs `node`), (2) generates the SDK via `tssdk::generate` and
//! materializes it through `tssdk::write_to_dir` into a UNIQUE temp subdir below `std::env::temp_dir()`
//! (the zero-dependency `std` path — no `tempfile` crate, threat T-05-03-SC), then runs the VERIFIED
//! typecheck against the vendored compiler:
//!   `node <repo>/tsextract/node_modules/typescript/bin/tsc --noEmit --strict --target es2022
//!    --module esnext --moduleResolution bundler --lib es2022,dom <each generated .ts>`
//! and asserts exit 0. The `--lib es2022,dom` is LOAD-BEARING: it declares the `fetch` global via
//! `lib.dom.d.ts`; omit `,dom` and TypeScript fails with `error TS2304: Cannot find name 'fetch'`
//! (RESEARCH Pitfall 3) — so the SDK can stay dependency-free (no `@types/node`).
//!
//! Hermeticity (CLAUDE.md rule 2 + ASVS): `current_dir` is the unique temp dir with NO nearby
//! `node_modules`/`tsconfig.json`, so no ambient `@types` or config leaks in; the typecheck reuses ONLY
//! the already-vendored `typescript` (Phase 4, committed lockfile) — no `npm install`. The harness also
//! greps every written `.ts` and asserts the generated SDK carries no third-party runtime import
//! (`axios`/`node-fetch`/`@types`/`from "http"`, the TSSDK-02 supply-chain gate, threat T-05-03-02).
//!
//! Requires `node` + the vendored `tsc`; skips gracefully (early return) if either is absent so a
//! non-Node environment never hard-fails the suite (mirrors how `tests/pysdk_compile.rs` skips without
//! `python3`).

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4 + ch.5); scope the allow to
// this test target so the workspace-wide RUST-04 deny stays intact for production code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The vendored `typescript` compiler, resolved relative to this crate's manifest dir. The SAME
/// compiler `tsextract` links (Phase 4, committed lockfile) — reused, never re-installed (T-05-03-SC).
const TSC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tsextract/node_modules/typescript/bin/tsc"
);

/// The `NestJS` fixture, resolved relative to this crate's manifest dir (mirrors the other tests). Its
/// IR comes through the Phase-4 `tsextract` path `build_graph` routes to — `node` must be present.
const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/nestjs-bookstore"
);

/// The generated SDK's package name (the single source of truth a `TsSdk` target derives — wired in
/// plan 02; passed through here as the `package` arg the same way).
const PACKAGE: &str = "bookstore";

/// The four files the `tssdk` bundle always frames (D-06 fixed alpha push order, mod.rs).
const SDK_FILES: [&str; 4] = ["client.ts", "errors.ts", "index.ts", "models.ts"];

/// Files emitted by the OpenAPI-generator compatibility profile.
const AXIOS_SDK_FILES: [&str; 6] = [
    "api.ts",
    "base.ts",
    "configuration.ts",
    "errors.ts",
    "index.ts",
    "models.ts",
];

/// Files emitted by the `typescript-fetch` OpenAPI-generator compatibility profile.
const FETCH_COMPAT_SDK_FILES: [&str; 4] =
    ["apis/index.ts", "index.ts", "models/index.ts", "runtime.ts"];

/// Whether the `node` + vendored `tsc` toolchain is available so this test skips gracefully if it is
/// absent. Checks BOTH `node` (drives tsextract AND tsc) and the vendored compiler file existing.
fn toolchain_available() -> bool {
    let node_ok = Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok();
    node_ok && Path::new(TSC).exists()
}

/// Create a UNIQUE temp subdir under `std::env::temp_dir()` (PID + nanosecond timestamp — no
/// user-supplied path component, threat T-05-03-03). No `tempfile` crate (T-05-03-SC); copied from the
/// `pysdk_compile` twin so the harnesses stay aligned.
fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-tssdk-compile-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

/// Run the VERIFIED `tsc --noEmit` typecheck over `ts_files` (each a path relative to / under `dir`),
/// with `current_dir` = `dir` (no nearby `node_modules` → no ambient `@types`/tsconfig leak, threat
/// T-05-03-03). Discrete `Command::new("node").args([...])` ONLY — NEVER a shell string (threat
/// T-05-03-01 / V13). A spawn failure (missing toolchain) maps to `TypeScriptToolchainMissing`; a
/// non-zero exit maps to the generic captured-stderr `GoBuild { code, stderr }` carrier (reused, no new
/// variant — the plan's interfaces note). The helper uses NO `unwrap`/`expect` on the subprocess
/// `Result` (no panic, threat T-05-03-04).
fn run_tsc(ts_files: &[&str], dir: &Path) -> Result<String, gnr8::CoreError> {
    // The `--lib es2022,dom` is LOAD-BEARING: lib.dom.d.ts declares the `fetch` global so the SDK needs
    // no `@types/node` (omit `,dom` → error TS2304: Cannot find name 'fetch', RESEARCH Pitfall 3).
    let mut args: Vec<&str> = vec![
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
    ];
    args.extend_from_slice(ts_files);

    let output = Command::new("node")
        .args(&args)
        .current_dir(dir)
        .output()
        // Spawn failure (e.g. node absent) → the dedicated toolchain-missing variant (error.rs:59).
        .map_err(|source| gnr8::CoreError::TypeScriptToolchainMissing { source })?;
    if !output.status.success() {
        // Reuse the generic captured-stderr carrier (no new error variant — the plan's interfaces note:
        // GoBuild is the generic exit-code+stderr carrier the harness reuses, T-05-03-04). tsc prints
        // diagnostics to stdout, so fold both streams into the carrier for a useful message.
        let mut captured = String::from_utf8_lossy(&output.stdout).into_owned();
        captured.push_str(&String::from_utf8_lossy(&output.stderr));
        return Err(gnr8::CoreError::GoBuild {
            code: output.status.code(),
            stderr: captured,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Materialize the generated SDK into a fresh temp dir as flat `<dir>/client.ts` etc., returning the
/// dir. Unlike the Python twin (which nests a `bookstore/` package), `tssdk::write_to_dir` writes the
/// four files FLAT (the bundle's fixed frame names), so there is no package subdir.
fn materialize_sdk() -> PathBuf {
    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 4 build_graph must succeed (requires node for the tsextract sidecar)");
    // `base_path` is the graph's single source of truth; pass it through exactly as a Pipeline would
    // (CLAUDE.md rules 3 & 4) — the same way pysdk_compile/the SDK targets take it.
    let bundle = gnr8::tssdk::generate(&graph, PACKAGE, &graph.base_path)
        .expect("tssdk::generate must succeed");
    let dir = unique_temp_dir("ok");
    gnr8::sdk::bundle::write_to_dir(&bundle, &dir).expect("write_to_dir must materialize the SDK");
    dir
}

#[expect(
    clippy::too_many_lines,
    reason = "the media graph is an explicit JSON fixture covering four content types"
)]
fn media_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "app",
          "operations": [
            {
              "id": "postText",
              "method": "POST",
              "path": "/text",
              "handler": "postText",
              "params": [],
              "request_body": { "ref_id": "dto.TextBody" },
              "request_body_required": true,
              "request_body_content_type": "text/plain",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.ts", "start_line": 1, "end_line": 1 }
            },
            {
              "id": "postForm",
              "method": "POST",
              "path": "/form",
              "handler": "postForm",
              "params": [],
              "request_body": { "ref_id": "dto.FormBody" },
              "request_body_required": true,
              "request_body_content_type": "application/x-www-form-urlencoded",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.ts", "start_line": 2, "end_line": 2 }
            },
            {
              "id": "postMultipart",
              "method": "POST",
              "path": "/multipart",
              "handler": "postMultipart",
              "params": [],
              "request_body": { "ref_id": "dto.MultipartBody" },
              "request_body_required": true,
              "request_body_content_type": "multipart/form-data",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.ts", "start_line": 3, "end_line": 3 }
            },
            {
              "id": "postBinary",
              "method": "POST",
              "path": "/binary",
              "handler": "postBinary",
              "params": [],
              "request_body": { "ref_id": "dto.UploadBytes" },
              "request_body_required": true,
              "request_body_content_type": "application/octet-stream",
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "main.ts", "start_line": 4, "end_line": 4 }
            }
          ],
          "schemas": [
            {
              "id": "dto.FormBody",
              "name": "FormBody",
              "body": { "type": "object", "of": [
                {
                  "json_name": "count",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "name",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "tags",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "array", "of": { "type": "primitive", "of": { "prim": "string" } } },
                  "description": null,
                  "example": null
                }
              ] },
              "enum_source_order": [],
              "provenance": { "file": "models.ts", "start_line": 1, "end_line": 1 }
            },
            {
              "id": "dto.MultipartBody",
              "name": "MultipartBody",
              "body": { "type": "object", "of": [
                {
                  "json_name": "file",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "bytes" } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "title",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "description": null,
                  "example": null
                },
                {
                  "json_name": "files",
                  "required": true,
                  "optional": false,
                  "nullable": false,
                  "schema": { "type": "array", "of": { "type": "primitive", "of": { "prim": "bytes" } } },
                  "description": null,
                  "example": null
                }
              ] },
              "enum_source_order": [],
              "provenance": { "file": "models.ts", "start_line": 2, "end_line": 2 }
            },
            {
              "id": "dto.TextBody",
              "name": "TextBody",
              "body": { "type": "primitive", "of": { "prim": "string" } },
              "enum_source_order": [],
              "provenance": { "file": "models.ts", "start_line": 3, "end_line": 3 }
            },
            {
              "id": "dto.UploadBytes",
              "name": "UploadBytes",
              "body": { "type": "primitive", "of": { "prim": "bytes" } },
              "enum_source_order": [],
              "provenance": { "file": "models.ts", "start_line": 4, "end_line": 4 }
            }
          ],
          "diagnostics": [],
          "base_path": "/",
          "title": "API",
          "security": []
        }"#,
    )
    .expect("media graph json")
}

fn materialize_media_sdk() -> PathBuf {
    let graph = media_graph();
    let bundle = gnr8::tssdk::generate(&graph, PACKAGE, &graph.base_path)
        .expect("media tssdk::generate must succeed");
    let dir = unique_temp_dir("media");
    gnr8::sdk::bundle::write_to_dir(&bundle, &dir)
        .expect("write_to_dir must materialize the media SDK");
    dir
}

fn materialize_split_sdk() -> PathBuf {
    use gnr8::sdk::prelude::SdkFileLayout;

    let mut graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 4 build_graph must succeed (requires node for the tsextract sidecar)");
    for op in &mut graph.operations {
        op.group = Some("Books".to_string());
    }
    let layout = SdkFileLayout::split()
        .operation_file_template("apis/api_{service_snake}.ts")
        .model_file_template("types/{schema_kebab}.ts");
    let bundle = gnr8::tssdk::generate_with_layout(&graph, PACKAGE, &graph.base_path, &layout)
        .expect("split tssdk::generate_with_layout must succeed");
    let dir = unique_temp_dir("split-ok");
    gnr8::sdk::bundle::write_to_dir(&bundle, &dir)
        .expect("write_to_dir must materialize the split SDK");
    dir
}

fn collect_ts_files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read generated SDK dir") {
            let entry = entry.expect("read generated SDK entry");
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.extension().is_some_and(|ext| ext == "ts") {
                out.push(
                    path.strip_prefix(root)
                        .expect("generated file under SDK dir")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }

    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort();
    files
}

fn materialize_axios_sdk() -> PathBuf {
    use gnr8::sdk::prelude::*;

    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 4 build_graph must succeed (requires node for the tsextract sidecar)");
    let root = unique_temp_dir("axios-root");
    let mut artifacts = Artifacts::new();
    TsSdk::new()
        .module("example.com/bookstore/sdk")
        .to("sdk")
        .profile(SdkProfile::openapi_generator_compat())
        .generate(&graph, &mut artifacts, &Cx::new(root.clone()))
        .expect("axios profile SDK generation must succeed");
    for artifact in artifacts.files() {
        let path = root.join(&artifact.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create artifact parent");
        }
        std::fs::write(path, &artifact.text).expect("write generated artifact");
    }
    write_axios_type_stub(&root.join("sdk"));
    root.join("sdk")
}

fn materialize_fetch_compat_sdk() -> PathBuf {
    use gnr8::sdk::prelude::*;

    let graph = gnr8::analyze::build_graph(FIXTURE_DIR)
        .expect("Phase 4 build_graph must succeed (requires node for the tsextract sidecar)");
    let root = unique_temp_dir("fetch-root");
    let mut artifacts = Artifacts::new();
    TsSdk::new()
        .module("example.com/bookstore/sdk")
        .to("sdk")
        .profile(SdkProfile::typescript_fetch_compat())
        .generate(&graph, &mut artifacts, &Cx::new(root.clone()))
        .expect("typescript-fetch profile SDK generation must succeed");
    for artifact in artifacts.files() {
        let path = root.join(&artifact.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create artifact parent");
        }
        std::fs::write(path, &artifact.text).expect("write generated artifact");
    }
    root.join("sdk")
}

fn write_axios_type_stub(dir: &Path) {
    let axios_dir = dir.join("node_modules/axios");
    std::fs::create_dir_all(&axios_dir).expect("create axios stub dir");
    std::fs::write(
        axios_dir.join("package.json"),
        r#"{"name":"axios","version":"1.0.0","types":"index.d.ts"}"#,
    )
    .expect("write axios package stub");
    std::fs::write(
        axios_dir.join("index.d.ts"),
        r"
export interface AxiosRequestConfig {
  method?: string;
  url?: string;
  params?: unknown;
  data?: unknown;
  headers?: unknown;
  validateStatus?: (status: number) => boolean;
  [key: string]: unknown;
}
export interface AxiosResponse<T = unknown> {
  status: number;
  headers: Record<string, unknown>;
  data: T;
}
export interface AxiosInstance {
  request<T = unknown>(config: AxiosRequestConfig): Promise<AxiosResponse<T>>;
}
declare const axios: AxiosInstance;
export default axios;
",
    )
    .expect("write axios type stub");
}

/// TSSDK-02 + the supply-chain gate: the generated SDK type-checks under `tsc --noEmit --strict --lib
/// es2022,dom` (exit 0) using ONLY the vendored compiler, and carries ZERO third-party runtime imports
/// (grepped over the written files — `axios`/`node-fetch`/`@types`/`from "http"`).
#[test]
fn generated_sdk_typechecks_with_vendored_tsc() {
    if !toolchain_available() {
        eprintln!("skipping tssdk_compile: node/tsc toolchain unavailable");
        return;
    }
    let dir = materialize_sdk();

    // The four production SDK files exist flat in the temp dir.
    for name in SDK_FILES {
        assert!(
            dir.join(name).exists(),
            "expected {name} in {}",
            dir.display()
        );
    }

    // Supply-chain assertion (TSSDK-02 / threat T-05-03-02): no third-party runtime/HTTP/types import
    // lands in the generated output — the SDK stands on the platform `fetch` + the bundled `lib.dom`.
    for name in SDK_FILES {
        let src = std::fs::read_to_string(dir.join(name)).expect("read generated .ts");
        for banned in ["axios", "node-fetch", "@types", "from \"http\""] {
            assert!(
                !src.contains(banned),
                "generated {name} must not contain a third-party runtime import ({banned}):\n{src}"
            );
        }
    }

    // The load-bearing typecheck: hand each generated .ts to the vendored tsc with discrete args and the
    // temp dir as current_dir (no ambient @types/tsconfig). Exit 0 == the SDK type-checks (TSSDK-02).
    let result = run_tsc(&SDK_FILES, &dir);
    assert!(
        result.is_ok(),
        "tsc --noEmit --strict --lib es2022,dom must type-check the generated SDK (exit 0): {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

#[test]
fn generated_sdk_media_request_bodies_typecheck_with_consumer() {
    if !toolchain_available() {
        eprintln!("skipping tssdk_compile media typecheck: node/tsc toolchain unavailable");
        return;
    }
    let dir = materialize_media_sdk();
    std::fs::write(
        dir.join("media_consumer.ts"),
        r#"
import { Client } from "./index";

async function smoke(client: Client): Promise<void> {
  await client.postText("hello");
	  await client.postForm({ name: "Ada", count: 3, tags: ["sdk", "media"] });
	  await client.postMultipart({
	    title: "Report",
	    file: new Uint8Array([1, 2, 3]),
	    files: [new Uint8Array([4, 5, 6]), new Uint8Array([7, 8, 9])],
	  });
  await client.postBinary(new Uint8Array([1, 2, 3]));
}

void smoke;
"#,
    )
    .expect("write media consumer");

    let ts_files = collect_ts_files(&dir);
    let ts_file_refs: Vec<&str> = ts_files.iter().map(String::as_str).collect();
    let result = run_tsc(&ts_file_refs, &dir);
    assert!(
        result.is_ok(),
        "media SDK and consumer must type-check (text + form + multipart + binary): {result:?}"
    );
    let client_src = std::fs::read_to_string(dir.join("client.ts")).expect("read media client.ts");
    assert!(
        client_src.contains("if (Array.isArray(value))")
            && client_src.contains("this._appendMultipartValue(form, key, item);"),
        "multipart helper must append array values as repeated parts:\n{client_src}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

#[test]
fn split_generated_sdk_with_group_facades_typechecks_with_vendored_tsc() {
    if !toolchain_available() {
        eprintln!("skipping split tssdk_compile: node/tsc toolchain unavailable");
        return;
    }
    let dir = materialize_split_sdk();
    let ts_files = collect_ts_files(&dir);
    let ts_file_refs: Vec<&str> = ts_files.iter().map(String::as_str).collect();

    assert!(
        ts_files.iter().any(|file| file == "apis/api_books.ts"),
        "expected split operation file in generated SDK: {ts_files:?}"
    );
    assert!(
        ts_files.iter().any(|file| file == "types/book-dto.ts"),
        "expected custom model file in generated SDK: {ts_files:?}"
    );

    let result = run_tsc(&ts_file_refs, &dir);
    assert!(
        result.is_ok(),
        "split SDK with grouped facades and custom file templates must type-check: {result:?}"
    );

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}

#[test]
fn openapi_generator_axios_profile_typechecks_with_test_local_axios_stub() {
    if !toolchain_available() {
        eprintln!("skipping tssdk axios compile: node/tsc toolchain unavailable");
        return;
    }
    let dir = materialize_axios_sdk();

    for name in AXIOS_SDK_FILES {
        assert!(
            dir.join(name).exists(),
            "expected {name} in {}",
            dir.display()
        );
    }
    let api = std::fs::read_to_string(dir.join("api.ts")).expect("read api.ts");
    assert!(
        api.contains("from \"axios\""),
        "axios profile must use axios imports:\n{api}"
    );
    let package = std::fs::read_to_string(dir.join("package.json")).expect("read package.json");
    assert!(
        package.contains("\"axios\""),
        "axios profile package.json must declare axios:\n{package}"
    );
    std::fs::write(
        dir.join("consumer.ts"),
        r#"
import { BookFormat, DefaultApiFactory } from "./index";

async function smoke(): Promise<void> {
  const api = DefaultApiFactory();
  const response = await api.listBooks({ genre: "fiction" });
  response.status.toFixed();
  response.headers;
  response.data.books;
  const format: BookFormat = BookFormat.Hardcover;
  void format;
}

void smoke;
"#,
    )
    .expect("write axios consumer smoke test");

    let mut ts_files = AXIOS_SDK_FILES.to_vec();
    ts_files.push("consumer.ts");
    let result = run_tsc(&ts_files, &dir);
    assert!(
        result.is_ok(),
        "axios profile must type-check against the test-local axios stub: {result:?}"
    );

    let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
}

#[test]
fn typescript_fetch_compat_profile_typechecks_with_raw_response_consumer() {
    if !toolchain_available() {
        eprintln!("skipping tssdk fetch compat compile: node/tsc toolchain unavailable");
        return;
    }
    let dir = materialize_fetch_compat_sdk();

    for name in FETCH_COMPAT_SDK_FILES {
        assert!(
            dir.join(name).exists(),
            "expected {name} in {}",
            dir.display()
        );
    }
    let runtime = std::fs::read_to_string(dir.join("runtime.ts")).expect("read runtime.ts");
    for snippet in [
        "export class BaseAPI",
        "export class Configuration",
        "export class JSONApiResponse<T>",
        "export class VoidApiResponse",
        "export class BlobApiResponse",
    ] {
        assert!(runtime.contains(snippet), "missing {snippet}:\n{runtime}");
    }

    std::fs::write(
        dir.join("consumer.ts"),
        r#"
import { Configuration, DefaultApi } from "./index";

async function smoke(): Promise<void> {
  const api = new DefaultApi(new Configuration({ basePath: "https://example.test" }));
  const raw = await api.listBooksRaw({ genre: "fiction" });
  raw.raw.status.toFixed();
  const value = await raw.value();
  value.books;
}

void smoke;
"#,
    )
    .expect("write fetch consumer smoke test");

    let mut ts_files = FETCH_COMPAT_SDK_FILES.to_vec();
    ts_files.push("consumer.ts");
    let result = run_tsc(&ts_files, &dir);
    assert!(
        result.is_ok(),
        "typescript-fetch profile must type-check with the raw response consumer: {result:?}"
    );

    let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
}

/// TSSDK-03: the materialized SDK is byte-identical across two independent generate->write runs
/// (deterministic output — identical input ⇒ byte-identical files, CLAUDE.md standing constraint).
#[test]
fn generated_sdk_is_byte_identical_across_two_runs() {
    if !toolchain_available() {
        eprintln!("skipping tssdk_compile determinism: node/tsc toolchain unavailable");
        return;
    }
    let dir_a = materialize_sdk();
    let dir_b = materialize_sdk();
    for name in SDK_FILES {
        let a = std::fs::read_to_string(dir_a.join(name)).expect("read run-a .ts");
        let b = std::fs::read_to_string(dir_b.join(name)).expect("read run-b .ts");
        assert_eq!(
            a, b,
            "two generate->write runs must produce byte-identical {name}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}

/// Threat T-05-03-04 / RUST-04: `tsc` over invalid TypeScript surfaces a captured-stderr `CoreError`
/// (carrying the exit code + captured diagnostics), never a panic in the `run_tsc` helper. Mirrors the
/// `pysdk_compile` twin's `invalid_python_compile_maps_to_captured_error_not_panic`.
#[test]
fn invalid_ts_typecheck_maps_to_captured_error_not_panic() {
    if !toolchain_available() {
        eprintln!("skipping tssdk_compile error-path: node/tsc toolchain unavailable");
        return;
    }
    let dir = unique_temp_dir("bad");
    // Deliberately invalid TypeScript — a type error under --strict must exit non-zero.
    let broken = dir.join("broken.ts");
    std::fs::write(&broken, "const n: number = \"not a number\";\n").expect("write broken.ts");

    let result = run_tsc(&["broken.ts"], &dir);
    match result {
        Err(gnr8::CoreError::GoBuild { code, stderr }) => {
            assert!(
                code != Some(0),
                "a failed typecheck must not report exit code 0"
            );
            assert!(
                !stderr.is_empty(),
                "the error must carry the captured diagnostics"
            );
        }
        other => panic!("expected a captured-stderr CoreError, got {other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir); // best-effort cleanup
}
