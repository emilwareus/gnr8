//! gnr8 generation lifecycle for the taskflow example — and a showcase of the POWER of code-as-config.
//!
//! THIS FILE IS THE CONFIG. There is no TOML, no YAML, no knobs file. It is an ordinary Rust binary
//! that composes a [`Pipeline`] out of four kinds of stage and hands it to the gnr8 runner. The host
//! (`gnr8 generate`) COMPILES AND RUNS this crate, then writes the artifacts it emits.
//!
//! The point of this example is that the built-in stages and YOUR OWN Rust compose freely — you are not
//! limited to a fixed set of flags. This lifecycle mixes:
//!
//!   * gnr8 built-ins — `GoGin` (source), `SetBasePath`/`SetTitle`/`ApplySecurity` (transforms),
//!     `OpenApi31`/`GoSdk` (targets), `Header::generated()` (post-process);
//!   * a USER-DEFINED `Transform` — [`DropDebugRoutes`], which edits the IR in Rust before generation;
//!   * a USER-DEFINED `Target` — [`ApiMarkdown`], a ~30-line generator that writes an `API.md` summary.
//!
//! Run it from the taskflow module root so `GoGin::new().inputs(["."])` analyzes the Go module here:
//!
//! ```sh
//! cd examples/taskflow
//! gnr8 generate                                   # the host compiles + runs this crate, writes output
//! # or, to see the raw bundle the child emits:
//! cargo run --quiet --manifest-path .gnr8/Cargo.toml -- __emit
//! ```

// `ApiGraph` and `CoreError` are not in the prelude (they are part of the deeper IR surface a custom
// stage touches); import them explicitly. The prelude carries everything else we compose.
use gnr8::graph::{ApiGraph, Type};
use gnr8::sdk::prelude::*;
use gnr8::CoreError;

// ---------------------------------------------------------------------------------------------------
// A custom Transform: edit the IR in Rust before generation.
// ---------------------------------------------------------------------------------------------------

/// Drop every internal `_debug` route from the API model.
///
/// The Go service registers a real `GET /tasks/_debug` diagnostics endpoint, but it should not appear
/// in the public OpenAPI document or the generated SDK. A `Transform` receives the IR by `&mut`, so we
/// just filter the operations — this is "edit the IR in code," the seam that replaces a config DSL.
/// Every later stage (OpenApi31, GoSdk, ApiMarkdown) sees the already-filtered model.
struct DropDebugRoutes;

impl Transform for DropDebugRoutes {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.operations.retain(|op| !op.path.contains("_debug"));
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// A custom Target: write your own generator in ~30 lines.
// ---------------------------------------------------------------------------------------------------

/// Emit an `API.md` Markdown summary of the API: a table of operations and a list of schemas.
///
/// This is the "write your own generator" showcase. A `Target` reads the frozen IR (`&ApiGraph`) and
/// writes files into [`Artifacts`] with `out.write(path, text)`; the host persists them like any other
/// output. Because the IR is already sorted (operations by path+method, schemas by id) and we build the
/// string deterministically, two runs produce byte-identical Markdown.
struct ApiMarkdown {
    /// Project-relative output path for the generated Markdown (e.g. `"generated/API.md"`).
    path: String,
}

impl Target for ApiMarkdown {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        let mut md = String::new();
        md.push_str(&format!("# {}\n\n", ir.title));
        md.push_str(&format!("Base path: `{}`\n\n", ir.base_path));

        md.push_str("## Operations\n\n");
        md.push_str("| Method | Path | Operation |\n");
        md.push_str("|--------|------|-----------|\n");
        for op in &ir.operations {
            // Render the absolute path (base_path + group-relative path), matching the OpenAPI document.
            let path = join_base(&ir.base_path, &op.path);
            md.push_str(&format!("| {} | `{}` | {} |\n", op.method, path, op.id));
        }

        md.push_str("\n## Schemas\n\n");
        for schema in &ir.schemas {
            // The neutral `Type` variant IS the schema's kind (the old string `kind` field was removed
            // when the IR became language-neutral in v2.0). Match exhaustively — a new variant is a
            // compile error here, never a silently-mislabeled schema (CLAUDE.md rule 3).
            let kind = match &schema.body {
                Type::Primitive(_) => "primitive",
                Type::WellKnown(_) => "well-known",
                Type::Array(_) => "array",
                Type::Map { .. } => "map",
                Type::Named(_) => "named",
                Type::Object(_) => "object",
                Type::Enum(_) => "enum",
                Type::Union(_) => "union",
                Type::Any {} => "any",
            };
            md.push_str(&format!("- `{}` ({})\n", schema.name, kind));
        }

        out.write(self.path.clone(), md);
        Ok(())
    }

    /// `API.md` is generated INTO the analyzed tree, so declare it as a loop-safety anchor: the pipeline
    /// excludes anything whose source lives under an output path, so a re-run never re-ingests its own
    /// generated files. (The built-in targets do the same for `openapi.yaml` and the SDK dir.)
    fn output_anchors(&self) -> Vec<String> {
        vec![self.path.clone()]
    }
}

/// Join the API base path with a source-derived operation path, collapsing the seam slash — the same
/// rule the OpenAPI lowering uses, so the Markdown paths match the spec (`/` + `/tasks` → `/tasks`,
/// `/` + `/tasks/{id}` → `/tasks/{id}`).
fn join_base(base: &str, relative: &str) -> String {
    let base = base.trim_end_matches('/');
    if relative == "/" {
        return format!("{base}/");
    }
    let suffix = relative.strip_prefix('/').unwrap_or(relative);
    format!("{base}/{suffix}")
}

// ---------------------------------------------------------------------------------------------------
// The pipeline: built-ins + your own Rust, composed in order.
// ---------------------------------------------------------------------------------------------------

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            // Source: read the Go + Gin service in this module (built-in).
            .source(GoGin::new().inputs(["."]))
            // Transforms: metadata the typed Go source can't express (built-ins) + our own edit.
            .transform(SetBasePath::new("/"))
            .transform(SetTitle::new("Taskflow API"))
            .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
            .transform(DropDebugRoutes) // <-- user-defined: drop the internal /tasks/_debug route
            // Targets: the standard OpenAPI + Go SDK (built-ins) AND our own Markdown generator.
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(GoSdk::new().module("example.com/taskflow/sdk").to("generated/sdk"))
            .target(ApiMarkdown {
                path: "generated/API.md".to_string(),
            }) // <-- user-defined: write API.md
            // Post-process: stamp the "generated by gnr8" banner on every .go file (built-in).
            .post(Header::generated()),
    )
}
