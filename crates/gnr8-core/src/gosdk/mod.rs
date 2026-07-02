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
    api_key_header_names, check_unique_schema_names, file_stem, model_file_name,
    operation_file_name, validate_sdk_base_path,
};
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::profile::SdkProfile;
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
    generate_files_with_profile(
        graph,
        package,
        base_path,
        layout,
        aliases,
        &SdkProfile::minimal(),
    )
}

pub(crate) fn generate_files_with_profile(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    profile: &SdkProfile,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    validate_sdk_base_path(base_path)?;
    check_unique_schema_names(graph, "Go SDK")?;

    if profile.is_go_openapi_generator_compat() {
        return generate_go_openapi_generator_compat_files(graph, package, base_path, aliases);
    }

    let mut files: Vec<SdkFile> = Vec::new();
    let auth_headers = api_key_header_names(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;
    let emit_compat_surface =
        profile.is_go_openapi_generator_compat() || aliases.has_source_prefix_aliases();
    let compat_options = emit::GoEmitOptions {
        compat_model_helpers: emit_compat_surface,
    };

    // Fixed leading files (sorted: client.go before errors.go).
    files.push(raw_go_file(
        "client.go",
        emit::emit_client(package, !auth_headers.is_empty()),
    ));
    files.push(raw_go_file("errors.go", emit::emit_errors(package)));
    if emit_compat_surface {
        files.push(raw_go_file(
            "compat_helpers.go",
            emit::emit_compat_helpers(package),
        ));
        files.push(raw_go_file(
            "compat_client.go",
            emit::emit_compat_client_surface(graph, package, base_path)?,
        ));
    }
    if !resolved_aliases.is_empty() {
        files.push(raw_go_file(
            "aliases.go",
            emit::emit_type_aliases(graph, package, &resolved_aliases, compat_options)?,
        ));
    }
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    if layout.is_split() {
        for op in &ops {
            let raw = emit::emit_operations_without_facades(graph, package, base_path, &[*op])?;
            let name = operation_file_name(layout, op, &format!("api_{}.go", file_stem(&op.id)))?;
            files.push(raw_go_file(name, raw));
        }
        if let Some(raw) = emit::emit_facades(graph, package, &ops)? {
            files.push(raw_go_file("facades.go", raw));
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
        let raw = emit::emit_operations(graph, package, base_path, &ops)?;
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

fn generate_go_openapi_generator_compat_files(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    aliases: &SdkTypeAliases,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    let resolved_aliases = aliases.resolve(graph)?;
    let compat_options = emit::GoEmitOptions {
        compat_model_helpers: true,
    };
    let mut files = vec![
        raw_go_file(
            "client.go",
            emit::emit_compat_api_client_file(graph, package)?,
        ),
        raw_go_file("configuration.go", emit::emit_compat_configuration(package)),
        raw_go_file("errors.go", emit::emit_compat_errors(package)),
        raw_go_file("utils.go", emit::emit_compat_utils(package)),
    ];
    for (service, ops) in emit::compat_operations_by_service(graph) {
        files.push(raw_go_file(
            format!("api_{}.go", file_stem(&service)),
            emit::emit_compat_api_file(graph, package, base_path, &service, &ops)?,
        ));
    }
    if !resolved_aliases.is_empty() {
        files.push(raw_go_file(
            "aliases.go",
            emit::emit_type_aliases(graph, package, &resolved_aliases, compat_options)?,
        ));
    }
    for schema in &graph.schemas {
        files.push(raw_go_file(
            format!("model_{}.go", file_stem(&schema.name)),
            emit::emit_model_schema_with_options(graph, package, schema, compat_options)?,
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

    use super::{
        generate, generate_files_with_layout, generate_files_with_profile, generate_with_layout,
    };
    use crate::graph::{ApiGraph, Param, Prim, Response, SecurityScheme, SourceSpan, Type};
    use crate::sdk::layout::SdkFileLayout;
    use crate::sdk::profile::SdkProfile;
    use crate::sdk::surface::SdkTypeAliases;

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
    fn split_layout_emits_group_facades_once() {
        if !gofmt_available() {
            eprintln!("skipping split facade test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Goals".to_string());
        }
        let out =
            generate_with_layout(&graph, "goalservice", "/goal", &SdkFileLayout::split()).unwrap();
        assert!(
            out.contains("// ==== gnr8:file facades.go ===="),
            "split layout should emit a dedicated facade file:\n{out}"
        );
        assert_eq!(out.matches("type GoalsAPI struct").count(), 1, "{out}");
        assert_eq!(
            out.matches("func (c *Client) Goals() *GoalsAPI").count(),
            1,
            "{out}"
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

    #[test]
    fn source_prefix_aliases_emit_grouped_go_compat_client_surface() {
        if !gofmt_available() {
            eprintln!("skipping compat client test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Goals".to_string());
        }
        let aliases = SdkTypeAliases::new().source_prefix_alias("dto.", "Dto");
        let files = generate_files_with_layout(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &aliases,
        )
        .unwrap();
        let compat = files
            .iter()
            .find(|file| file.name == "compat_client.go")
            .map(|file| file.contents.as_str())
            .expect("compat_client.go should be emitted");

        for snippet in [
            "func NewConfiguration() *Configuration",
            "func NewAPIClient(cfg *Configuration) *APIClient",
            "GoalsAPI   *GoalsAPIService",
            "func (a *GoalsAPIService) ListGoals(ctx context.Context) ApiListGoalsRequest",
            "func (r ApiListGoalsRequest) Aggregation(aggregation any) ApiListGoalsRequest",
            "func (r ApiCreateGoalRequest) GoalInput(goalInput any) ApiCreateGoalRequest",
            "func (r ApiListGoalsRequest) Execute() (*ListGoalsOutput, *http.Response, error)",
        ] {
            assert!(compat.contains(snippet), "missing {snippet}:\n{compat}");
        }
    }

    #[test]
    fn go_openapi_generator_profile_emits_compat_client_surface_without_aliases() {
        if !gofmt_available() {
            eprintln!("skipping Go profile compat client test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Goals".to_string());
        }
        let files = generate_files_with_profile(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();
        for name in [
            "api_goals.go",
            "client.go",
            "configuration.go",
            "errors.go",
            "utils.go",
        ] {
            assert!(
                files.iter().any(|file| file.name == name),
                "missing {name}: {files:#?}"
            );
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test constructs a complete binary route and verifies the generated compatibility surface"
    )]
    fn go_openapi_generator_profile_emits_grouped_requests_binary_and_scoped_headers() {
        if !gofmt_available() {
            eprintln!("skipping Go profile scoped compat test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        graph.security = vec![
            SecurityScheme {
                id: "ActiveSchoolAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: "X-Plint-School-Id".to_string(),
                global: false,
            },
            SecurityScheme {
                id: "CSRFAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: "X-CSRF-Token".to_string(),
                global: false,
            },
        ];
        graph.operations[0].id = "getCourseworkSubmissionAttachment".to_string();
        graph.operations[0].handler = "getCourseworkSubmissionAttachment".to_string();
        graph.operations[0].group = Some("Coursework".to_string());
        graph.operations[0].method = "GET".to_string();
        graph.operations[0].path =
            "/coursework/assignments/{assignmentId}/submissions/{studentPersonId}/attachment"
                .to_string();
        graph.operations[0].params = vec![
            Param {
                name: "assignmentId".to_string(),
                location: "path".to_string(),
                required: true,
                schema: Type::Primitive(Prim::String),
                default: None,
                provenance: SourceSpan {
                    file: "/root/http.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            },
            Param {
                name: "studentPersonId".to_string(),
                location: "path".to_string(),
                required: true,
                schema: Type::Primitive(Prim::String),
                default: None,
                provenance: SourceSpan {
                    file: "/root/http.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            },
        ];
        graph.operations[0].request_body = None;
        graph.operations[0].responses = vec![Response {
            status: 200,
            body: None,
            body_kind: "binary".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            content_types: vec!["application/octet-stream".to_string()],
        }];
        graph.operations[0].security = vec!["ActiveSchoolAuth".to_string(), "CSRFAuth".to_string()];
        graph.operations[1].security.clear();

        let files = generate_files_with_profile(
            &graph,
            "plintsdk",
            "/v1",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();

        for name in [
            "api_coursework.go",
            "client.go",
            "configuration.go",
            "errors.go",
            "utils.go",
        ] {
            assert!(
                files.iter().any(|file| file.name == name),
                "missing {name}: {files:#?}"
            );
        }

        let api = files
            .iter()
            .find(|file| file.name == "api_coursework.go")
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "func (a *CourseworkAPIService) GetCourseworkSubmissionAttachment(ctx context.Context, assignmentID any, studentPersonID any) ApiGetCourseworkSubmissionAttachmentRequest",
            "func (r ApiGetCourseworkSubmissionAttachmentRequest) Execute() ([]byte, *http.Response, error)",
            "compatApplyAPIKey(req, r.ctx, \"ActiveSchoolAuth\", \"X-Plint-School-Id\")",
            "compatApplyAPIKey(req, r.ctx, \"CSRFAuth\", \"X-CSRF-Token\")",
            "req.Header.Set(\"Accept\", \"application/octet-stream\")",
            "localVarReturnValue = localVarBody",
            "&GenericOpenAPIError{body: localVarBody, error: resp.Status}",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }
        for forbidden in [
            "req.Header.Get(\"Authorization\") != \"\"",
            "req.Header.Set(\"X-CSRF-Token\", req.Header.Get(\"Authorization\"))",
            "req.Header.Set(\"X-Plint-School-Id\", req.Header.Get(\"Authorization\"))",
        ] {
            assert!(!api.contains(forbidden), "forbidden {forbidden}:\n{api}");
        }

        let utils = files
            .iter()
            .find(|file| file.name == "utils.go")
            .unwrap()
            .contents
            .as_str();
        assert!(
            utils.contains("func parameterAddToHeaderOrQuery("),
            "{utils}"
        );

        let errors = files
            .iter()
            .find(|file| file.name == "errors.go")
            .unwrap()
            .contents
            .as_str();
        assert!(
            errors.contains("type GenericOpenAPIError struct"),
            "{errors}"
        );
    }
}
