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

mod emit;
mod gofmt;

use crate::graph::{ApiGraph, Operation};
use crate::sdk::bundle::{SdkBundle, SdkFile};
use crate::sdk::emit_common::{
    api_key_header_name, file_stem, model_file_name, operation_file_name,
};
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::surface::SdkTypeAliases;

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
    generate_with_layout(graph, package, base_path, &SdkFileLayout::compact())
}

/// Generate the Go SDK with a configurable file layout.
///
/// # Errors
///
/// Returns the same errors as [`generate`].
pub fn generate_with_layout(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
) -> Result<String, crate::CoreError> {
    let aliases = SdkTypeAliases::default();
    let files = generate_files_with_layout(graph, package, base_path, layout, &aliases)?;
    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}

pub(crate) fn generate_files_with_layout(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    let mut files: Vec<SdkFile> = Vec::new();
    let auth_header = api_key_header_name(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;
    let compat_options = emit::GoEmitOptions {
        compat_model_helpers: aliases.has_source_prefix_aliases(),
    };

    // Fixed leading files (sorted: client.go before errors.go).
    files.push(raw_go_file(
        "client.go",
        emit::emit_client(package, auth_header.as_deref()),
    ));
    files.push(raw_go_file("errors.go", emit::emit_errors(package)));
    if !resolved_aliases.is_empty() {
        files.push(raw_go_file(
            "aliases.go",
            emit::emit_type_aliases(graph, package, &resolved_aliases, compat_options)?,
        ));
    }
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    if layout.is_split() {
        for op in &ops {
            let raw =
                emit::emit_operations(graph, package, base_path, &[*op], auth_header.as_deref())?;
            let name = operation_file_name(layout, op, &format!("api_{}.go", file_stem(&op.id)))?;
            files.push(raw_go_file(name, raw));
        }
        for schema in &graph.schemas {
            let raw = emit::emit_model_schema_with_options(graph, package, schema, compat_options)?;
            let name = model_file_name(
                layout,
                schema,
                &format!("model_{}.go", file_stem(&schema.name)),
            )?;
            files.push(raw_go_file(name, raw));
        }
    } else {
        // All operations go into a single generic `operations.go` resource surface. Tags were an
        // annotation fact and have been removed (CLAUDE.md rules 1 & 3), so there is no per-tag grouping;
        // the file name is generic (not the package/fixture name) so it never overfits to one service.
        let raw = emit::emit_operations(graph, package, base_path, &ops, auth_header.as_deref())?;
        files.push(raw_go_file("operations.go", raw));

        // Trailing models.go.
        files.push(raw_go_file(
            "models.go",
            emit::emit_models_with_options(graph, package, compat_options)?,
        ));
    }

    let mut files = gofmt::gofmt_files(files)?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

/// Wrap a raw emitted file as a named [`SdkFile`] before batched formatting.
fn raw_go_file(name: impl Into<String>, raw: impl Into<String>) -> SdkFile {
    SdkFile {
        name: name.into(),
        contents: raw.into(),
    }
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code. These tests require the Go
    // toolchain (generate runs gofmt) and skip gracefully if it is absent.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{generate, generate_with_layout};
    use crate::graph::ApiGraph;
    use crate::sdk::layout::SdkFileLayout;

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
              "schema": { "type": "primitive", "of": { "prim": "string" } },
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
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "dto.ListGoalsOutput" } } ],
          "span": { "file": "/root/http.go", "start_line": 3, "end_line": 3 }
        }
      ],
      "schemas": [
        {
          "id": "dto.CommandMessage", "name": "CommandMessage",
          "body": { "type": "object", "of": [
            { "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/c.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.CreateGoalInput", "name": "CreateGoalInput",
          "body": { "type": "object", "of": [
            { "json_name": "name", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null },
            { "json_name": "targetDirection", "required": false, "optional": true, "nullable": true,
              "schema": { "type": "named", "of": "dto.TargetDirection" },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.HttpError", "name": "HttpError",
          "body": { "type": "object", "of": [
            { "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/c.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.ListGoalsOutput", "name": "ListGoalsOutput",
          "body": { "type": "object", "of": [
            { "json_name": "total", "required": false, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.TargetDirection", "name": "TargetDirection",
          "body": { "type": "enum", "of": ["gte","lte"] },
          "span": { "file": "/root/c.go", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "dto.UpdateGoalInput", "name": "UpdateGoalInput",
          "body": { "type": "object", "of": [
            { "json_name": "name", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 3, "end_line": 3 }
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

    #[test]
    fn split_layout_emits_one_operation_file_and_one_model_file_per_item() {
        if !gofmt_available() {
            eprintln!("skipping split layout test: gofmt unavailable");
            return;
        }
        let out = generate_with_layout(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
        )
        .unwrap();
        for marker in [
            "// ==== gnr8:file api_create_goal.go ====",
            "// ==== gnr8:file api_list_goals.go ====",
            "// ==== gnr8:file api_update_goal.go ====",
            "// ==== gnr8:file model_create_goal_input.go ====",
            "// ==== gnr8:file model_target_direction.go ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
        assert!(
            !out.contains("// ==== gnr8:file operations.go ===="),
            "split layout must not emit the compact operations file:\n{out}"
        );
        assert!(
            !out.contains("// ==== gnr8:file models.go ===="),
            "split layout must not emit the compact models file:\n{out}"
        );
    }

    #[test]
    fn split_layout_can_place_operation_and_model_files_in_configured_dirs() {
        if !gofmt_available() {
            eprintln!("skipping custom split layout test: gofmt unavailable");
            return;
        }
        let layout = SdkFileLayout::split()
            .operation_dir("apis")
            .model_dir("types");
        let out = generate_with_layout(&sample_graph(), "goalservice", "/goal", &layout).unwrap();
        for marker in [
            "// ==== gnr8:file apis/api_create_goal.go ====",
            "// ==== gnr8:file types/model_create_goal_input.go ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
    }
}
