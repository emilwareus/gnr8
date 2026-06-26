//! TypeScript SDK generation seam (Phase 5): generates a dependency-free TypeScript SDK from the API
//! graph.
//!
//! [`generate`] turns the Phase-2 [`crate::graph::ApiGraph`] into a single deterministic,
//! dependency-free TypeScript SDK bundle String (D-06): an `index.ts` re-export surface, a `client.ts`
//! (an injectable platform-`fetch`-backed `Client` plus one method per operation), a typed `errors.ts`
//! (`ApiError extends Error`), and a `models.ts` (`export interface` request/response models +
//! string-literal-union named enums + `export type` aliases).
//!
//! This is the structural twin of [`crate::pysdk`], MINUS the Python-only workarounds (required-first
//! field ordering, the `from __future__` header, PEP-484 forward-ref aliases, the f-string `safe=''`
//! trick): TypeScript `?:` is order-free, `type` aliases are order-independent, and template literals
//! impose no backslash restriction. Each file is framed into a [`bundle::SdkBundle`] with stable file
//! markers; the pipeline is byte-identical across runs and never panics (RUST-04). [`write_to_dir`]
//! materializes the same framing.

mod emit;

use crate::graph::{ApiGraph, Operation};
use crate::sdk::bundle::{SdkBundle, SdkFile};
use crate::sdk::emit_common::{api_key_header_name, file_in_dir, file_stem, model_file_name};
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::surface::SdkTypeAliases;

/// Generate the TypeScript SDK as a deterministic, dependency-free multi-file bundle String (D-06,
/// TSSDK-01).
///
/// Emits `client.ts` (the `fetch`-backed `Client` + one method per operation), `errors.ts` (typed
/// `ApiError`), `index.ts` (re-exports), and `models.ts` (`interface` models + literal-union enums +
/// `type` aliases) in a FIXED alpha push order, then frames them into a single [`bundle::SdkBundle`]
/// String. Generating twice over the same graph is byte-identical (TSSDK-03). There is NO `gofmt`-style
/// normalization step (the generated TypeScript is already correct) and NO computed import header.
///
/// `package` is the SDK's package name (derived from the `TsSdk` target's module path, the single source
/// of truth — wired in plan 02). `base_path` is the API base/mount path joined to each operation's
/// group-relative path in the emitted request URLs — the SAME single source of truth (the graph's
/// `base_path`) the `OpenAPI` lowering and the Go/Python SDKs take it from (CLAUDE.md rules 3 & 4).
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] for an un-representable graph fact (a dangling `$ref`, an inline
/// object, a path whose templated tokens do not match its declared path params, a duplicate schema name,
/// or a `fmt` write error folded by the emitters' `sink`).
pub fn generate(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
) -> Result<String, crate::CoreError> {
    generate_with_layout(graph, package, base_path, SdkFileLayout::compact())
}

/// Generate the TypeScript SDK with a configurable file layout.
pub fn generate_with_layout(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: SdkFileLayout,
) -> Result<String, crate::CoreError> {
    let files =
        generate_files_with_layout(graph, package, base_path, layout, SdkTypeAliases::default())?;
    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}

pub(crate) fn generate_files_with_layout(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: SdkFileLayout,
    aliases: SdkTypeAliases,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    let mut files: Vec<SdkFile> = Vec::new();
    let auth_header = api_key_header_name(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;

    // Fixed alpha push order: client.ts, errors.ts, index.ts, models.ts — the D-06 frame order the
    // bundle locks. client.ts is the client skeleton followed by the operation methods.
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let model_dir = layout.model_dir_ref().unwrap_or("models");
    let mut client =
        emit::emit_client_with_models(package, model_dir.trim_matches('/'), auth_header.as_deref());
    client.push_str(&emit::emit_operations(
        graph,
        package,
        base_path,
        &ops,
        auth_header.as_deref(),
    )?);
    files.push(SdkFile {
        name: "client.ts".to_string(),
        contents: client,
    });

    files.push(SdkFile {
        name: "errors.ts".to_string(),
        contents: emit::emit_errors(package),
    });

    files.push(SdkFile {
        name: "index.ts".to_string(),
        contents: emit::emit_index_with_models(
            graph,
            package,
            model_dir.trim_matches('/'),
            &resolved_aliases,
        )?,
    });

    if layout.is_split() {
        files.push(SdkFile {
            name: file_in_dir(Some(model_dir), "index.ts"),
            contents: emit::emit_models_index(graph, &resolved_aliases)?,
        });
        for schema in &graph.schemas {
            let default_name =
                file_in_dir(Some(model_dir), &format!("{}.ts", file_stem(&schema.name)));
            let name = if layout.model_file_template_ref().is_some() {
                model_file_name(&layout, schema, &format!("{}.ts", file_stem(&schema.name)))?
            } else {
                default_name
            };
            files.push(SdkFile {
                name,
                contents: emit::emit_model_schema(graph, schema)?,
            });
        }
        for alias in &resolved_aliases {
            files.push(SdkFile {
                name: file_in_dir(Some(model_dir), &format!("{}.ts", file_stem(&alias.alias))),
                contents: emit::emit_model_alias(alias),
            });
        }
    } else {
        files.push(SdkFile {
            name: "models.ts".to_string(),
            contents: emit::emit_models_with_aliases(graph, package, &resolved_aliases)?,
        });
    }

    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

/// Split a generated SDK bundle String into its `(file_name, contents)` pairs.
///
/// Wraps the crate-private [`bundle::parse`] framing so the lifecycle layer can enumerate the SDK's
/// per-file outputs without re-implementing the marker split. Single source of truth for the framing —
/// the same one [`write_to_dir`] uses. (Consumed by the `TsSdk` target in `sdk::builtins`.)
#[cfg(test)]
pub(crate) fn split_bundle(bundle: &str) -> Vec<(String, String)> {
    crate::sdk::bundle::parse(bundle)
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code. Unlike the Go twin, these tests
    // require NO toolchain — `generate` is pure string emission with no `tsc`/`node` subprocess (the
    // hermetic typecheck lands in plan 03's tests/tssdk_compile.rs).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{generate, generate_with_layout, split_bundle};
    use crate::graph::ApiGraph;
    use crate::sdk::bundle::write_to_dir;
    use crate::sdk::layout::SdkFileLayout;

    /// A facts document covering one body POST and one query GET plus the request/response models +
    /// a named enum — enough to assert the four-file bundle shape and determinism without a toolchain.
    const SAMPLE: &[u8] = br#"{
      "module": "app",
      "routes": [
        {
          "method": "POST", "path": "/books", "handler": "createBook",
          "operation_id": "createBook", "params": [],
          "request_body": { "ref_id": "app.models.Book" },
          "responses": [
            { "status": 201, "body": { "ref_id": "app.models.CreatedMessage" } }
          ],
          "span": { "file": "/root/main.ts", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listBooks",
          "operation_id": "listBooks",
          "params": [
            { "name": "cursor", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/main.ts", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "app.models.Book" } } ],
          "span": { "file": "/root/main.ts", "start_line": 2, "end_line": 2 }
        }
      ],
      "schemas": [
        {
          "id": "app.models.Book", "name": "Book",
          "body": { "type": "object", "of": [
            { "json_name": "format", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "named", "of": "app.models.BookFormat" },
              "description": null, "example": null },
            { "json_name": "title", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "app.models.BookFormat", "name": "BookFormat",
          "body": { "type": "enum", "of": ["hardcover", "paperback"] },
          "span": { "file": "/root/m.ts", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "app.models.CreatedMessage", "name": "CreatedMessage",
          "body": { "type": "object", "of": [
            { "json_name": "id", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.ts", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    #[test]
    fn generate_returns_ok_with_the_four_file_markers_in_fixed_order() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        let order: Vec<usize> = [
            "// ==== gnr8:file client.ts ====",
            "// ==== gnr8:file errors.ts ====",
            "// ==== gnr8:file index.ts ====",
            "// ==== gnr8:file models.ts ====",
        ]
        .iter()
        .map(|m| out.find(m).unwrap_or_else(|| panic!("missing {m}:\n{out}")))
        .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "markers must appear in the fixed client/errors/index/models order:\n{out}"
        );
    }

    #[test]
    fn generate_is_byte_identical_across_two_runs() {
        let graph = sample_graph();
        assert_eq!(
            generate(&graph, "bookstore", "/").unwrap(),
            generate(&graph, "bookstore", "/").unwrap(),
            "two generate runs must be byte-identical"
        );
    }

    #[test]
    fn generated_client_contains_the_operation_methods_and_models_the_enum() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        assert!(out.contains("async createBook(body: models.Book)"), "{out}");
        assert!(out.contains("async listBooks(cursor?: string)"), "{out}");
        assert!(
            out.contains("export type BookFormat = \"hardcover\" | \"paperback\";"),
            "{out}"
        );
        assert!(out.contains("export interface Book {"), "{out}");
    }

    #[test]
    fn split_bundle_round_trips_to_the_four_files() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["client.ts", "errors.ts", "index.ts", "models.ts"]
        );
        // The marker line must never appear inside a materialized file's contents.
        for (_, contents) in &files {
            assert!(
                !contents.contains("// ==== gnr8:file"),
                "marker leaked into a file"
            );
        }
    }

    #[test]
    fn write_to_dir_rejects_an_unsafe_frame_name() {
        // A hand-forged bundle whose frame name escapes the target dir must be refused (T-05-01-01).
        let evil = "// ==== gnr8:file ../escape.ts ====\nexport const x = 1;\n";
        let dir = std::env::temp_dir();
        let err = write_to_dir(evil, &dir).unwrap_err();
        assert!(
            err.to_string().contains("unsafe name"),
            "unsafe frame name must be rejected: {err}"
        );
    }

    #[test]
    fn write_to_dir_materializes_the_four_files() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("gnr8-tssdk-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_to_dir(&out, &dir).unwrap();
        for name in ["client.ts", "errors.ts", "index.ts", "models.ts"] {
            assert!(dir.join(name).is_file(), "missing materialized {name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_layout_emits_models_directory_with_barrel_file() {
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", SdkFileLayout::split())
            .unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "client.ts",
                "errors.ts",
                "index.ts",
                "models/book.ts",
                "models/book_format.ts",
                "models/created_message.ts",
                "models/index.ts",
            ]
        );
        assert!(
            !names.contains(&"models.ts"),
            "split layout must not emit compact models.ts"
        );

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("gnr8-tssdk-split-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_to_dir(&out, &dir).unwrap();
        assert!(dir.join("models/book.ts").is_file());
        assert!(dir.join("models/index.ts").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_layout_can_place_models_in_a_configured_directory() {
        let layout = SdkFileLayout::split().model_dir("schemas");
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", layout).unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"schemas/book.ts"));
        assert!(names.contains(&"schemas/index.ts"));
        assert!(
            out.contains("import * as models from \"./schemas\";"),
            "{out}"
        );
        assert!(out.contains("} from \"./schemas\";"), "{out}");
    }
}
