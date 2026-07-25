//! Multi-tag Go SDK compile gate: shared wire helpers must be emitted once.
//!
//! Split `PerTag` layouts previously redeclared `wireParameterPair` in every `api_*.go` file.
//! This fixture generates into an empty directory and requires `go test ./...` to pass.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use gnr8::sdk::prelude::*;

fn temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-go-wire-helpers-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_artifacts(out: &Artifacts, root: &Path) {
    for file in out.files() {
        let path = root.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create artifact parent");
        }
        std::fs::write(path, &file.text).expect("write artifact");
    }
}

fn multi_tag_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "wire.test",
          "operations": [
            {
              "id": "listCatalog",
              "method": "GET",
              "path": "/catalog",
              "handler": "listCatalog",
              "group": "Catalog",
              "params": [
                {
                  "name": "itemTypes",
                  "location": "query",
                  "required": true,
                  "schema": {
                    "type": "array",
                    "of": { "type": "primitive", "of": { "prim": "string" } }
                  },
                  "style": "form",
                  "explode": true,
                  "provenance": { "file": "a.go", "start_line": 1, "end_line": 1 }
                },
                {
                  "name": "statuses",
                  "location": "query",
                  "required": false,
                  "schema": {
                    "type": "array",
                    "of": { "type": "primitive", "of": { "prim": "string" } }
                  },
                  "style": "form",
                  "explode": true,
                  "provenance": { "file": "a.go", "start_line": 2, "end_line": 2 }
                },
                {
                  "name": "page",
                  "location": "query",
                  "required": false,
                  "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
                  "provenance": { "file": "a.go", "start_line": 3, "end_line": 3 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 1, "end_line": 3 }
            },
            {
              "id": "getCatalogItem",
              "method": "GET",
              "path": "/catalog/{id}",
              "handler": "getCatalogItem",
              "group": "Catalog",
              "params": [
                {
                  "name": "id",
                  "location": "path",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "a.go", "start_line": 4, "end_line": 4 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 4, "end_line": 4 }
            },
            {
              "id": "listGoals",
              "method": "GET",
              "path": "/goals",
              "handler": "listGoals",
              "group": "Goals",
              "params": [
                {
                  "name": "tags",
                  "location": "query",
                  "required": true,
                  "schema": {
                    "type": "array",
                    "of": { "type": "primitive", "of": { "prim": "string" } }
                  },
                  "style": "form",
                  "explode": true,
                  "provenance": { "file": "a.go", "start_line": 5, "end_line": 5 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 5, "end_line": 5 }
            },
            {
              "id": "getGoal",
              "method": "GET",
              "path": "/goals/{id}",
              "handler": "getGoal",
              "group": "Goals",
              "params": [
                {
                  "name": "id",
                  "location": "path",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "a.go", "start_line": 6, "end_line": 6 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 6, "end_line": 6 }
            },
            {
              "id": "listIntegrations",
              "method": "GET",
              "path": "/integrations",
              "handler": "listIntegrations",
              "group": "Integrations",
              "params": [
                {
                  "name": "kinds",
                  "location": "query",
                  "required": true,
                  "schema": {
                    "type": "array",
                    "of": { "type": "primitive", "of": { "prim": "string" } }
                  },
                  "style": "form",
                  "explode": true,
                  "provenance": { "file": "a.go", "start_line": 7, "end_line": 7 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 7, "end_line": 7 }
            },
            {
              "id": "getIntegration",
              "method": "GET",
              "path": "/integrations/{id}",
              "handler": "getIntegration",
              "group": "Integrations",
              "params": [
                {
                  "name": "id",
                  "location": "path",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "a.go", "start_line": 8, "end_line": 8 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 8, "end_line": 8 }
            },
            {
              "id": "listJobs",
              "method": "GET",
              "path": "/jobs",
              "handler": "listJobs",
              "group": "Jobs",
              "params": [
                {
                  "name": "states",
                  "location": "query",
                  "required": true,
                  "schema": {
                    "type": "array",
                    "of": { "type": "primitive", "of": { "prim": "string" } }
                  },
                  "style": "form",
                  "explode": true,
                  "provenance": { "file": "a.go", "start_line": 9, "end_line": 9 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 9, "end_line": 9 }
            },
            {
              "id": "getJob",
              "method": "GET",
              "path": "/jobs/{id}",
              "handler": "getJob",
              "group": "Jobs",
              "params": [
                {
                  "name": "id",
                  "location": "path",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "a.go", "start_line": 10, "end_line": 10 }
                }
              ],
              "request_body": null,
              "responses": [{ "status": 204, "body": null }],
              "provenance": { "file": "a.go", "start_line": 10, "end_line": 10 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/",
          "title": "Wire Helpers",
          "security": []
        }"#,
    )
    .expect("multi-tag graph must deserialize")
}

#[test]
fn split_per_tag_go_sdk_emits_wire_helpers_once_and_compiles() {
    if Command::new("go")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("skipping: go toolchain unavailable");
        return;
    }

    let ir = multi_tag_graph();
    let root = temp_dir("empty");
    let sdk_dir = root.join("go-sdk");
    std::fs::create_dir_all(&sdk_dir).unwrap();
    assert!(sdk_dir.read_dir().unwrap().next().is_none());

    let mut out = Artifacts::new();
    GoSdk::new()
        .module("example.com/generated/sdk")
        .to("go-sdk")
        .layout(SdkFileLayout::split().operations_per_tag())
        .generate(&ir, &mut out, &Cx::new(&root))
        .expect("generate Go SDK");
    write_artifacts(&out, &root);

    let helpers = std::fs::read_to_string(sdk_dir.join("wire_helpers.go"))
        .expect("wire_helpers.go must exist for multi-tag wire-heavy APIs");
    assert!(
        helpers.contains("type wireParameterPair struct"),
        "shared helpers must define wireParameterPair once"
    );

    let mut pair_defs = 0usize;
    for entry in std::fs::read_dir(&sdk_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("go") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        pair_defs += text.matches("type wireParameterPair struct").count();
    }
    assert_eq!(
        pair_defs, 1,
        "wireParameterPair must be declared exactly once across the package"
    );

    let status = Command::new("go")
        .args(["test", "./..."])
        .current_dir(&sdk_dir)
        .output()
        .expect("spawn go test");
    assert!(
        status.status.success(),
        "go test failed:\n{}\n{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    let _ = std::fs::remove_dir_all(root);
}
