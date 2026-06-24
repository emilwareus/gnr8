//! Go SDK generation seam (Phase 3): generates a Go SDK from the API graph.
//!
//! [`generate`] turns the Phase-2 [`crate::graph::ApiGraph`] into a single deterministic, `gofmt`-clean
//! Go SDK bundle String (D-06): one functional-options `client.go`, one typed `errors.go`, one
//! tag-grouped `<tag>.go` per sorted tag, and one `models.go`. Each file is emitted by [`emit`]
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
/// Emits `client.go` (functional-options `Client`), `errors.go` (typed `APIError`), one `<tag>.go` per
/// sorted tag (tag-grouped, `context.Context`-first methods on `*Client`), and `models.go` (request/
/// response structs + enum newtypes), pipes each through `gofmt`, and frames them into a single
/// [`bundle::SdkBundle`] String. Generating twice over the same graph is byte-identical (T-03-02-03).
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] for an un-representable graph fact (dangling `$ref`, unknown
/// `kind`), [`crate::CoreError::GoFmt`] if `gofmt` rejects emitted Go, or
/// [`crate::CoreError::GoToolchainMissing`] if `gofmt` cannot be spawned.
pub fn generate(graph: &ApiGraph) -> Result<String, crate::CoreError> {
    let mut files: Vec<SdkFile> = Vec::new();

    // Fixed leading files (sorted: client.go before errors.go).
    files.push(go_file("client.go", &emit::emit_client())?);
    files.push(go_file("errors.go", &emit::emit_errors())?);

    // One operations file per tag, tags sorted lexically (Pitfall 4).
    for (tag, ops) in group_by_tag(graph) {
        let file_name = format!("{}.go", tag.to_ascii_lowercase());
        let raw = emit::emit_operations(graph, &tag, &ops)?;
        files.push(go_file(&file_name, &raw)?);
    }

    // Trailing models.go.
    files.push(go_file("models.go", &emit::emit_models(graph)?)?);

    // The bundle's fixed sorted order is established by push order above (client, errors, <tags>,
    // models) — exactly the D-06 frame order the snapshot locks.
    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}

/// `gofmt` a raw emitted file and wrap it as a named [`SdkFile`].
fn go_file(name: &str, raw: &str) -> Result<SdkFile, crate::CoreError> {
    Ok(SdkFile {
        name: name.to_string(),
        contents: gofmt::gofmt(raw)?,
    })
}

/// Group operations by tag, returning `(tag, ops)` pairs with tags sorted lexically.
///
/// An operation's tag is its first (already-sorted) tag; an untagged operation inherits the lexically-
/// first tag present anywhere in the graph, or the fixed package name if the graph carries no tags at
/// all. This deterministic rule keeps the single-resource fixture's four operations (two tagged
/// `Goals`, two untagged) in one `goals.go` file, matching `expected/sdk/goals.go`. Operations within a
/// tag preserve graph order (already sorted by `(path, method)` — Pitfall 4). Uses `Vec<(K, V)>`, never
/// a `HashMap`, so the grouping is byte-stable.
fn group_by_tag(graph: &ApiGraph) -> Vec<(String, Vec<&Operation>)> {
    // The default tag for untagged ops: the lexically-first tag in the graph, else the package name.
    let default_tag = graph
        .operations
        .iter()
        .flat_map(|op| op.tags.iter())
        .min()
        .map_or_else(|| emit::PACKAGE.to_string(), Clone::clone);

    let mut groups: Vec<(String, Vec<&Operation>)> = Vec::new();
    for op in &graph.operations {
        let tag = op
            .tags
            .first()
            .cloned()
            .unwrap_or_else(|| default_tag.clone());
        if let Some((_, ops)) = groups.iter_mut().find(|(t, _)| t == &tag) {
            ops.push(op);
        } else {
            groups.push((tag, vec![op]));
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    groups
}

/// Materialize an [`SdkBundle`]'s framed files to `dir/<name>` (consumed by 03-03's compile test).
///
/// File names are program-controlled (the fixed `client.go`/`errors.go`/`<tag>.go`/`models.go` set —
/// threat T-03-03 temp-dir hygiene; no untrusted path is joined). The bundle is re-parsed through the
/// shared [`bundle::parse`] framing so the on-disk files match the snapshot byte-for-byte.
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] if any file cannot be written.
#[allow(dead_code)] // consumed by 03-03's compile test; wired here for that plan.
pub(crate) fn write_to_dir(
    bundle: &SdkBundle,
    dir: &std::path::Path,
) -> Result<(), crate::CoreError> {
    let framed = bundle.to_string();
    for (name, contents) in bundle::parse(&framed) {
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

    /// A facts document covering both tagged (`Goals`) and untagged operations plus the four fixture
    /// request/response models + the `TargetDirection` enum — enough to assert the bundle shape without
    /// the live fixture. Mirrors the real graph's relevant subset.
    const SAMPLE: &[u8] = br#"{
      "module": "github.com/acme/svc",
      "routes": [
        {
          "method": "POST", "path": "/", "router_path": null, "handler": "createGoal",
          "operation_id": null, "summary": null, "tags": [], "secured": true,
          "security_schemes": [], "params": [],
          "request_body": { "ref_id": "dto.CreateGoalInput" },
          "responses": [
            { "status": 201, "body": { "ref_id": "dto.CommandMessage" }, "description": null },
            { "status": 400, "body": { "ref_id": "dto.HttpError" }, "description": null }
          ],
          "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "PUT", "path": "/{uuid}", "router_path": "/{uuid}", "handler": "updateGoal",
          "operation_id": "goalUuidPut", "summary": "Update", "tags": ["Goals"], "secured": true,
          "security_schemes": ["ApiKeyAuth"],
          "params": [
            { "name": "uuid", "location": "path", "required": true,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "enum_values": [], "description": null,
              "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } }
          ],
          "request_body": { "ref_id": "dto.UpdateGoalInput" },
          "responses": [ { "status": 200, "body": { "ref_id": "dto.CommandMessage" }, "description": null } ],
          "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
        },
        {
          "method": "GET", "path": "/list", "router_path": "/list", "handler": "listGoals",
          "operation_id": null, "summary": "List", "tags": ["Goals"], "secured": true,
          "security_schemes": ["ApiKeyAuth"],
          "params": [
            { "name": "aggregation", "location": "query", "required": true,
              "schema": { "kind": "string", "format": null, "items": null, "ref_id": null, "additional_properties": null },
              "enum_values": ["count","sum"], "description": "agg",
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "dto.ListGoalsOutput" }, "description": "ok" } ],
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
        let out = generate(&sample_graph()).unwrap();
        for marker in [
            "// ==== gnr8:file client.go ====",
            "// ==== gnr8:file errors.go ====",
            "// ==== gnr8:file goals.go ====",
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
            generate(&graph).unwrap(),
            generate(&graph).unwrap(),
            "two generate runs must be byte-identical"
        );
    }

    #[test]
    fn generated_models_contain_the_request_response_models_and_enum() {
        if !gofmt_available() {
            eprintln!("skipping models test: gofmt unavailable");
            return;
        }
        let out = generate(&sample_graph()).unwrap();
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
        let out = generate(&sample_graph()).unwrap();
        assert!(
            out.contains("func (c *Client) CreateGoal(ctx context.Context"),
            "CreateGoal must take ctx first:\n{out}"
        );
    }
}
