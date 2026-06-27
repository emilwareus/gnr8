//! Python SDK generation from the API graph.
//!
//! [`generate`] turns the Phase-2 [`crate::graph::ApiGraph`] into a single deterministic,
//! Python SDK bundle String (D-06): an `__init__.py` re-export surface, a `client.py`
//! (an injectable `urllib.request.OpenerDirector`-backed `Client` plus one method per operation), a
//! typed `errors.py` (`ApiError`), and model files (`pydantic.BaseModel` by default, or stdlib
//! dataclasses when explicitly configured, plus `enum.Enum` named enums).
//!
//! This is the structural twin of [`crate::gosdk`], MINUS the `gofmt` normalization step: Python has no
//! stdlib formatter, so [`emit`] produces already-correct significant-whitespace Python directly. Each
//! file is framed into a [`bundle::SdkBundle`] with stable file markers; the pipeline is byte-identical
//! across runs and never panics (RUST-04). [`write_to_dir`] materializes the same framing.

mod emit;

use crate::graph::{ApiGraph, Operation};
use crate::sdk::bundle::{SdkBundle, SdkFile};
use crate::sdk::emit_common::{api_key_header_name, file_stem, model_file_name};
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::model_style::PyModelStyle;
use crate::sdk::surface::SdkTypeAliases;

/// Generate the Python SDK as a deterministic multi-file bundle String (D-06, PYSDK-01).
///
/// Emits `__init__.py` (re-exports), `client.py` (the `urllib`-backed `Client` + one method per
/// operation), `errors.py` (typed `ApiError`), and model files (Pydantic v2 by default, dataclasses
/// when configured), then frames them into a single [`bundle::SdkBundle`] String. Generating twice over
/// the same graph is byte-identical (PYSDK-03). There is NO `gofmt`-style normalization step (Python
/// has no stdlib formatter) — the emitters produce correct significant-whitespace Python directly.
///
/// `package` is the SDK's Python package name (derived from the `PySdk` target's module path, the single
/// source of truth — wired in plan 03-02). `base_path` is the API base/mount path joined to each
/// operation's group-relative path in the emitted request URLs — the SAME single source of truth (the
/// graph's `base_path`) the `OpenAPI` lowering and the Go SDK take it from (CLAUDE.md rules 3 & 4).
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] for an un-representable graph fact (a dangling `$ref`, a path
/// whose templated tokens do not match its declared path params, or a `fmt` write error folded by the
/// emitters' `sink`).
pub fn generate(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
) -> Result<String, crate::CoreError> {
    generate_with_options(
        graph,
        package,
        base_path,
        &SdkFileLayout::compact(),
        PyModelStyle::default(),
        &SdkTypeAliases::default(),
    )
}

/// Generate the Python SDK with a configurable file layout.
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
    generate_with_options(
        graph,
        package,
        base_path,
        layout,
        PyModelStyle::default(),
        &SdkTypeAliases::default(),
    )
}

/// Generate the Python SDK with configurable file layout and model style.
///
/// # Errors
///
/// Returns the same errors as [`generate`], plus configuration errors for invalid compatibility
/// aliases.
pub fn generate_with_options(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    model_style: PyModelStyle,
    aliases: &SdkTypeAliases,
) -> Result<String, crate::CoreError> {
    let files =
        generate_files_with_options(graph, package, base_path, layout, model_style, aliases)?;
    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}

pub(crate) fn generate_files_with_options(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    model_style: PyModelStyle,
    aliases: &SdkTypeAliases,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    let mut files: Vec<SdkFile> = Vec::new();
    let auth_header = api_key_header_name(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;

    // Fixed sorted push order (alpha): __init__.py, client.py, errors.py, models.py — the D-06 frame
    // order the bundle locks. client.py is the client skeleton followed by the operation methods.
    let model_dir = layout.model_dir_ref().unwrap_or("models");
    let model_module = model_dir.trim_matches('/').replace('/', ".");

    files.push(SdkFile {
        name: "__init__.py".to_string(),
        contents: emit::emit_init_with_models(graph, package, &model_module),
    });

    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let mut client =
        emit::emit_client_with_models(package, &model_module, model_style, auth_header.as_deref());
    client.push_str(&emit::emit_operations_with_style(
        graph,
        package,
        base_path,
        &ops,
        model_style,
    )?);
    files.push(SdkFile {
        name: "client.py".to_string(),
        contents: client,
    });

    files.push(SdkFile {
        name: "errors.py".to_string(),
        contents: emit::emit_errors(package),
    });

    if layout.is_split() {
        files.push(SdkFile {
            name: crate::sdk::emit_common::file_in_dir(Some(model_dir), "__init__.py"),
            contents: emit::emit_models_init(graph, &resolved_aliases),
        });
        for schema in &graph.schemas {
            let default_name = crate::sdk::emit_common::file_in_dir(
                Some(model_dir),
                &format!("{}.py", file_stem(&schema.name)),
            );
            let name = if layout.model_file_template_ref().is_some() {
                model_file_name(layout, schema, &format!("{}.py", file_stem(&schema.name)))?
            } else {
                default_name
            };
            files.push(SdkFile {
                name,
                contents: emit::emit_model_schema(graph, schema, model_style)?,
            });
        }
        for alias in &resolved_aliases {
            files.push(SdkFile {
                name: crate::sdk::emit_common::file_in_dir(
                    Some(model_dir),
                    &format!("{}.py", file_stem(&alias.alias)),
                ),
                contents: emit::emit_model_alias(alias),
            });
        }
    } else {
        files.push(SdkFile {
            name: "models.py".to_string(),
            contents: emit::emit_models_with_style_and_aliases(
                graph,
                package,
                model_style,
                &resolved_aliases,
            )?,
        });
    }

    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

/// Split a generated SDK bundle String into its `(file_name, contents)` pairs.
///
/// Wraps the crate-private [`bundle::parse`] framing so the lifecycle layer can enumerate the SDK's
/// per-file outputs without re-implementing the marker split. Single source of truth for the framing —
/// the same one [`write_to_dir`] uses. (Consumed by the `PySdk` target in `sdk::builtins`.)
#[cfg(test)]
pub(crate) fn split_bundle(bundle: &str) -> Vec<(String, String)> {
    crate::sdk::bundle::parse(bundle)
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code. Unlike the Go twin, these tests
    // require NO toolchain — `generate` is pure string emission with no `python3` subprocess.
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
          "span": { "file": "/root/main.py", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listBooks",
          "operation_id": "listBooks",
          "params": [
            { "name": "cursor", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/main.py", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "app.models.Book" } } ],
          "span": { "file": "/root/main.py", "start_line": 2, "end_line": 2 }
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
          "span": { "file": "/root/m.py", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "app.models.BookFormat", "name": "BookFormat",
          "body": { "type": "enum", "of": ["hardcover", "paperback"] },
          "span": { "file": "/root/m.py", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "app.models.CreatedMessage", "name": "CreatedMessage",
          "body": { "type": "object", "of": [
            { "json_name": "id", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.py", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    #[test]
    fn generate_returns_ok_with_the_four_file_markers() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        for marker in [
            "// ==== gnr8:file __init__.py ====",
            "// ==== gnr8:file client.py ====",
            "// ==== gnr8:file errors.py ====",
            "// ==== gnr8:file models.py ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
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
        assert!(out.contains("def create_book(self, body: Book)"), "{out}");
        assert!(out.contains("def list_books(self, cursor=None)"), "{out}");
        assert!(out.contains("class BookFormat(str, enum.Enum):"), "{out}");
        assert!(out.contains("class Book(BaseModel):"), "{out}");
        assert!(out.contains("from pydantic import BaseModel"), "{out}");
    }

    #[test]
    fn split_bundle_round_trips_to_the_four_files() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["__init__.py", "client.py", "errors.py", "models.py"]
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
        // A hand-forged bundle whose frame name escapes the target dir must be refused (T-03-01-01).
        let evil = "// ==== gnr8:file ../escape.py ====\npass\n";
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
        let dir = std::env::temp_dir().join(format!("gnr8-pysdk-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_to_dir(&out, &dir).unwrap();
        for name in ["__init__.py", "client.py", "errors.py", "models.py"] {
            assert!(dir.join(name).is_file(), "missing materialized {name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_layout_emits_models_package_with_init_file() {
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", &SdkFileLayout::split())
            .unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "__init__.py",
                "client.py",
                "errors.py",
                "models/__init__.py",
                "models/book.py",
                "models/book_format.py",
                "models/created_message.py",
            ]
        );
        assert!(
            !names.contains(&"models.py"),
            "split layout must not emit compact models.py"
        );

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("gnr8-pysdk-split-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_to_dir(&out, &dir).unwrap();
        assert!(dir.join("models/book.py").is_file());
        assert!(dir.join("models/__init__.py").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_layout_can_place_models_in_a_configured_package_directory() {
        let layout = SdkFileLayout::split().model_dir("schemas");
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", &layout).unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"schemas/book.py"));
        assert!(names.contains(&"schemas/__init__.py"));
        assert!(
            out.contains("from .schemas import *"),
            "client.py should import the configured model package:\n{out}"
        );
        assert!(
            out.contains("from .schemas import ("),
            "__init__.py should re-export from the configured model package:\n{out}"
        );
    }
}
