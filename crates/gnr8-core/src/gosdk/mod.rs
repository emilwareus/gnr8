//! Go SDK generation seam (Phase 3): generates a Go SDK from the API graph.
//!
//! [`generate`] turns the Phase-2 [`crate::graph::ApiGraph`] into a single deterministic, `gofmt`-clean
//! Go SDK bundle String (D-06): one functional-options `client.go`, one typed `errors.go`, one generic
//! `operations.go` resource surface, and one `models.go`. Tags were an annotation fact and have been
//! removed (CLAUDE.md rules 1 & 3), so the SDK is a single operations surface rather than per-tag files.
//! The package name is supplied by the caller (derived from the `GoSdk` target's module path, the
//! single source of truth — see [`crate::sdk::builtins::GoSdk`]). Each file is emitted by [`emit`]
//! (`format!`-based, no template engine — D-05), normalized through the real `gofmt` ([`gofmt`]), and
//! framed into an [`bundle::SdkBundle`] with stable file markers. The pipeline is byte-identical across
//! runs and never panics (RUST-04); [`write_to_dir`] materializes the same framing for 03-03's compile
//! test.

mod bundle;
mod emit;
mod gofmt;

use crate::graph::{ApiGraph, Operation};
use bundle::{SdkBundle, SdkFile};

/// Generate the Go SDK as a deterministic, `gofmt`-clean multi-file bundle String (D-06, SDK-01..04).
///
/// Emits `client.go` (functional-options `Client`), `errors.go` (typed `APIError`), one generic
/// `operations.go` (`context.Context`-first methods on `*Client`), and `models.go` (request/response
/// structs + enum newtypes), pipes each through `gofmt`, and frames them into a single
/// [`bundle::SdkBundle`] String. Generating twice over the same graph is byte-identical (T-03-02-03).
///
/// `package` is the SDK's Go package name — derived from the `GoSdk` target's module path (the single
/// source of truth) via [`crate::sdk::builtins::GoSdk`]; it appears in every file's `package` clause.
/// `base_path` is the API base/mount path joined to each operation's group-relative path in the emitted
/// request URLs — the SAME single source of truth (the graph's `base_path`, set by a `SetBasePath`
/// transform) the `OpenAPI` lowering takes it from (CLAUDE.md rules 3 & 4), so the SDK and the spec
/// agree on the prefix.
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] for an un-representable graph fact (dangling `$ref`, unknown
/// `kind`), [`crate::CoreError::GoFmt`] if `gofmt` rejects emitted Go, or
/// [`crate::CoreError::GoToolchainMissing`] if `gofmt` cannot be spawned.
pub fn generate(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
) -> Result<String, crate::CoreError> {
    let mut files: Vec<SdkFile> = Vec::new();

    // Fixed leading files (sorted: client.go before errors.go).
    files.push(go_file("client.go", &emit::emit_client(package))?);
    files.push(go_file("errors.go", &emit::emit_errors(package))?);

    // All operations go into a single generic `operations.go` resource surface. Tags were an
    // annotation fact and have been removed (CLAUDE.md rules 1 & 3), so there is no per-tag grouping;
    // the file name is generic (not the package/fixture name) so it never overfits to one service.
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let raw = emit::emit_operations(graph, package, base_path, &ops)?;
    files.push(go_file("operations.go", &raw)?);

    // Trailing models.go.
    files.push(go_file("models.go", &emit::emit_models(graph, package)?)?);

    // The bundle's fixed sorted order is established by push order above (client, errors,
    // operations, models) — exactly the D-06 frame order the snapshot locks.
    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}

/// Split a generated SDK bundle String into its `(file_name, contents)` pairs.
///
/// Wraps the crate-private [`bundle::parse`] framing so the lifecycle layer can enumerate the SDK's
/// per-file outputs (to hash + ownership-track each file) without re-implementing the marker split
/// or reaching into the private `bundle` submodule. Single source of truth for the framing — the
/// same one [`write_to_dir`] uses. The caller is responsible for the frame-name path-safety check
/// when materializing (the names are program-controlled; see [`write_to_dir`]).
pub(crate) fn split_bundle(bundle: &str) -> Vec<(String, String)> {
    bundle::parse(bundle)
}

/// `gofmt` a raw emitted file and wrap it as a named [`SdkFile`].
fn go_file(name: &str, raw: &str) -> Result<SdkFile, crate::CoreError> {
    Ok(SdkFile {
        name: name.to_string(),
        contents: gofmt::gofmt(raw)?,
    })
}

/// Materialize a generated SDK bundle String's framed files to `dir/<name>` (03-03's compile test).
///
/// Takes the public [`generate`] output (the file-marker-framed bundle String) rather than the
/// crate-private [`bundle::SdkBundle`] so an out-of-crate integration test (`tests/sdk_compile.rs`)
/// can call it directly. File names are program-controlled — they come from the fixed
/// `client.go`/`errors.go`/`<tag>.go`/`models.go` frame markers, never from untrusted input — and are
/// joined onto the caller's program-controlled temp `dir` (threat T-03-03 temp-dir hygiene). The
/// bundle is split through the shared [`bundle::parse`] framing so the on-disk files match the
/// snapshot byte-for-byte.
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] if a frame name is empty/contains a path separator (so no
/// frame can escape `dir`) or if any file cannot be written.
pub fn write_to_dir(bundle: &str, dir: &std::path::Path) -> Result<(), crate::CoreError> {
    for (name, contents) in bundle::parse(bundle) {
        // Defense-in-depth: the frame names are program-generated, but reject anything that is not a
        // plain file name so a malformed bundle can never traverse out of `dir` (T-03-03).
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(crate::CoreError::SdkGen {
                message: format!("refusing to write SDK file with unsafe name {name:?}"),
            });
        }
        let path = dir.join(&name);
        std::fs::write(&path, contents).map_err(|err| crate::CoreError::SdkGen {
            message: format!("failed to write SDK file {}: {err}", path.display()),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code. These tests require the Go
    // toolchain (generate runs gofmt) and skip gracefully if it is absent.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::generate;
    use crate::graph::ApiGraph;

    /// A facts document (code-first shape — no annotation facts) covering three operations plus the
    /// fixture request/response models + the code-defined `TargetDirection` enum — enough to assert the
    /// bundle shape without the live fixture. Mirrors the real graph's relevant subset.
    const SAMPLE: &[u8] = br#"{
      "module": "github.com/acme/svc",
      "routes": [
        {
          "method": "POST", "path": "/", "handler": "createGoal",
          "operation_id": "createGoal", "params": [],
          "request_body": { "ref_id": "dto.CreateGoalInput" },
          "responses": [
            { "status": 201, "body": { "ref_id": "dto.CommandMessage" } },
            { "status": 400, "body": { "ref_id": "dto.HttpError" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "PUT", "path": "/{uuid}", "handler": "updateGoal",
          "operation_id": "updateGoal",
          "params": [
            { "name": "uuid", "location": "path", "required": true,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } }
          ],
          "request_body": { "ref_id": "dto.UpdateGoalInput" },
          "responses": [ { "status": 200, "body": { "ref_id": "dto.CommandMessage" } } ],
          "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listGoals",
          "operation_id": "listGoals",
          "params": [
            { "name": "aggregation", "location": "query", "required": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "dto.ListGoalsOutput" } } ],
          "span": { "file": "/root/http.go", "start_line": 3, "end_line": 3 }
        }
      ],
      "schemas": [
        {
          "id": "dto.CommandMessage", "name": "CommandMessage", "kind": "object",
          "fields": [
            { "json_name": "message", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/c.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.CreateGoalInput", "name": "CreateGoalInput", "kind": "object",
          "fields": [
            { "json_name": "name", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null },
            { "json_name": "targetDirection", "required": false, "optional": true,
              "schema": { "kind": "ref", "format": null, "items": null, "ref_id": "dto.TargetDirection", "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.HttpError", "name": "HttpError", "kind": "object",
          "fields": [
            { "json_name": "message", "required": true, "optional": false,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/c.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.ListGoalsOutput", "name": "ListGoalsOutput", "kind": "object",
          "fields": [
            { "json_name": "total", "required": false, "optional": false,
              "schema": { "kind": "integer", "format": "int64", "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/g.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.TargetDirection", "name": "TargetDirection", "kind": "enum",
          "fields": [], "enum_values": ["gte","lte"],
          "span": { "file": "/root/c.go", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "dto.UpdateGoalInput", "name": "UpdateGoalInput", "kind": "object",
          "fields": [
            { "json_name": "name", "required": false, "optional": true,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "description": null, "example": null }
          ],
          "enum_values": [], "span": { "file": "/root/g.go", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// Whether `gofmt` is available so toolchain-dependent tests skip gracefully.
    fn gofmt_available() -> bool {
        std::process::Command::new("gofmt")
            .arg("-h")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    }

    #[test]
    fn generate_returns_ok_with_the_four_file_markers() {
        if !gofmt_available() {
            eprintln!("skipping generate test: gofmt unavailable");
            return;
        }
        let out = generate(&sample_graph(), "goalservice", "/goal").unwrap();
        for marker in [
            "// ==== gnr8:file client.go ====",
            "// ==== gnr8:file errors.go ====",
            "// ==== gnr8:file operations.go ====",
            "// ==== gnr8:file models.go ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
    }

    #[test]
    fn generate_is_byte_identical_across_two_runs() {
        if !gofmt_available() {
            eprintln!("skipping determinism test: gofmt unavailable");
            return;
        }
        let graph = sample_graph();
        assert_eq!(
            generate(&graph, "goalservice", "/goal").unwrap(),
            generate(&graph, "goalservice", "/goal").unwrap(),
            "two generate runs must be byte-identical"
        );
    }

    #[test]
    fn generated_models_contain_the_request_response_models_and_enum() {
        if !gofmt_available() {
            eprintln!("skipping models test: gofmt unavailable");
            return;
        }
        let out = generate(&sample_graph(), "goalservice", "/goal").unwrap();
        for ty in [
            "type CreateGoalInput struct",
            "type UpdateGoalInput struct",
            "type ListGoalsOutput struct",
            "type TargetDirection string",
        ] {
            assert!(out.contains(ty), "missing {ty}:\n{out}");
        }
    }

    #[test]
    fn generated_goals_file_has_ctx_first_create_goal_method() {
        if !gofmt_available() {
            eprintln!("skipping ops test: gofmt unavailable");
            return;
        }
        let out = generate(&sample_graph(), "goalservice", "/goal").unwrap();
        assert!(
            out.contains("func (c *Client) CreateGoal(ctx context.Context"),
            "CreateGoal must take ctx first:\n{out}"
        );
    }
}
