//! The built-in pipeline stages — thin wrappers over the existing deterministic core functions.
//!
//! Every stage here reproduces a knob that used to live in `.gnr8/config.toml`, now expressed as a
//! composable Rust value. CRITICAL (CLAUDE.md rules 2 & 3): these NEVER re-implement extraction,
//! lowering, or SDK emission, and they NEVER add a second source for a fact or a fallback path. A
//! source calls [`crate::analyze::build_graph`]; a target reads the graph metadata a transform set
//! and calls the existing [`crate::lower::to_openapi`] / [`crate::gosdk::generate`]; a transform
//! mutates the one graph. One deterministic path per fact.

// User-facing prose dense with proper nouns (Gin, OpenAPI, SDK, apiKey, ...); allow doc_markdown
// module-wide (mirrors the rest of the framework surface).
#![allow(clippy::doc_markdown)]

use super::{Artifacts, Cx, PostProcess, Source, Target, Transform};
use crate::graph::{ApiGraph, SecurityScheme};
use crate::CoreError;

// ---------------------------------------------------------------------------------------------------
// Source
// ---------------------------------------------------------------------------------------------------

/// The Go + Gin source: wraps [`crate::analyze::build_graph`] (the goextract subprocess driver).
///
/// `inputs` are project-relative source directories; for now exactly ONE is supported (multi-input
/// fan-in is a documented later stage), and a different count is a clear typed error rather than a
/// silent first-wins. The single input is resolved against [`Cx::project_root`] so a relative `"."`
/// analyzes the project root, not the process cwd.
#[derive(Debug, Default, Clone)]
pub struct GoGin {
    inputs: Vec<String>,
}

impl GoGin {
    /// A Go + Gin source with no inputs yet (configure with [`GoGin::inputs`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source input directories (project-relative). Exactly one is supported for now.
    #[must_use]
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}

impl Source for GoGin {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        // Exactly one input dir for now (mirrors the lifecycle single-input PoC restriction): reject
        // zero or many with a clear typed error rather than silently analyzing the first (D-02).
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message:
                        "GoGin source has no inputs — call .inputs([\".\"]) with one source dir"
                            .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "GoGin source lists {} inputs, but multi-input analysis is not yet supported \
                         — configure exactly one source dir",
                        many.len()
                    ),
                });
            }
        };
        // Resolve the input against the project root so a relative input analyzes the PROJECT, not the
        // process cwd (an absolute input is left as-is by `Path::join`). This matches the lifecycle's
        // input-resolution and keeps span provenance relative to the same root.
        let resolved = cx.project_root.join(input);
        let input_arg = resolved.to_string_lossy();
        crate::analyze::build_graph(&input_arg)
    }
}

/// The FastAPI (Python) source: wraps [`crate::analyze::build_graph`] (the pyextract subprocess
/// driver), exactly like [`GoGin`] wraps goextract.
///
/// `inputs` are project-relative source directories; for now exactly ONE is supported, and a
/// different count is a clear typed error rather than a silent first-wins. The single input is
/// resolved against [`Cx::project_root`]. This Source does NOT pick the language — it calls the SAME
/// [`crate::analyze::build_graph`], which detects Python by scanning the target (CLAUDE.md rule 3):
/// one deterministic path per fact, never a per-Source extraction fork.
#[derive(Debug, Default, Clone)]
pub struct FastApi {
    inputs: Vec<String>,
}

impl FastApi {
    /// A FastAPI source with no inputs yet (configure with [`FastApi::inputs`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source input directories (project-relative). Exactly one is supported for now.
    #[must_use]
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}

impl Source for FastApi {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        // Exactly one input dir for now: reject zero or many with a clear typed error rather than
        // silently analyzing the first (mirrors GoGin).
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message:
                        "FastApi source has no inputs — call .inputs([\".\"]) with one source dir"
                            .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "FastApi source lists {} inputs, but multi-input analysis is not yet \
                         supported — configure exactly one source dir",
                        many.len()
                    ),
                });
            }
        };
        // Resolve against the project root so a relative input analyzes the PROJECT, not the process
        // cwd. The SAME build_graph the Go source calls — language dispatch is by target detection.
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph(&resolved.to_string_lossy())
    }
}

/// The Flask (Python) source: wraps [`crate::analyze::build_graph`] (the pyextract subprocess
/// driver), a verbatim twin of [`FastApi`]/[`GoGin`] differing only in the error proper noun.
///
/// `inputs` are project-relative source directories; exactly ONE is supported for now. Like every
/// other source it calls the SAME [`crate::analyze::build_graph`] — language is detected from the
/// target, never from which Source was used (CLAUDE.md rule 3).
#[derive(Debug, Default, Clone)]
pub struct Flask {
    inputs: Vec<String>,
}

impl Flask {
    /// A Flask source with no inputs yet (configure with [`Flask::inputs`]).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the source input directories (project-relative). Exactly one is supported for now.
    #[must_use]
    pub fn inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inputs = inputs.into_iter().map(Into::into).collect();
        self
    }
}

impl Source for Flask {
    fn load(&self, cx: &Cx) -> Result<ApiGraph, CoreError> {
        let input = match self.inputs.as_slice() {
            [single] => single,
            [] => {
                return Err(CoreError::Config {
                    message:
                        "Flask source has no inputs — call .inputs([\".\"]) with one source dir"
                            .to_string(),
                });
            }
            many => {
                return Err(CoreError::Config {
                    message: format!(
                        "Flask source lists {} inputs, but multi-input analysis is not yet \
                         supported — configure exactly one source dir",
                        many.len()
                    ),
                });
            }
        };
        let resolved = cx.project_root.join(input);
        crate::analyze::build_graph(&resolved.to_string_lossy())
    }
}

// ---------------------------------------------------------------------------------------------------
// Transforms
// ---------------------------------------------------------------------------------------------------

/// Set [`ApiGraph::base_path`] — the API base/mount path joined to every group-relative operation
/// path (replaces the `base_path` TOML knob).
#[derive(Debug, Clone)]
pub struct SetBasePath {
    base_path: String,
}

impl SetBasePath {
    /// Build the transform with the given base path (e.g. `"/books"`).
    #[must_use]
    pub fn new(base_path: impl Into<String>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }
}

impl Transform for SetBasePath {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.base_path.clone_from(&self.base_path);
        Ok(())
    }
}

/// Set [`ApiGraph::title`] — the OpenAPI document title (`info.title`) (replaces the `title` knob).
#[derive(Debug, Clone)]
pub struct SetTitle {
    title: String,
}

impl SetTitle {
    /// Build the transform with the given title (e.g. `"Bookstore API"`).
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl Transform for SetTitle {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.title.clone_from(&self.title);
        Ok(())
    }
}

/// Push a security scheme onto [`ApiGraph::security`] — the single source of truth for the generated
/// `security` requirement + `components.securitySchemes` (replaces the `[[security.schemes]]` knob,
/// CLAUDE.md rule 4).
#[derive(Debug, Clone)]
pub struct ApplySecurity {
    scheme: SecurityScheme,
}

impl ApplySecurity {
    /// An `apiKey`-in-`header` scheme: `id` is the OpenAPI scheme id (e.g. `"ApiKeyAuth"`),
    /// `header_name` is the credential header (e.g. `"X-API-Key"`).
    #[must_use]
    pub fn api_key(id: impl Into<String>, header_name: impl Into<String>) -> Self {
        Self {
            scheme: SecurityScheme {
                id: id.into(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: header_name.into(),
            },
        }
    }
}

impl Transform for ApplySecurity {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        ir.security.push(self.scheme.clone());
        Ok(())
    }
}

/// Rename an operation by id: remap `from`'s `operation.id` to `to` (replaces a `[naming.operations]`
/// entry). Reuses the existing [`crate::lifecycle::apply_naming`] logic so the rename semantics (and
/// the `$ref`-rewrite guarantees) stay identical to the host path.
#[derive(Debug, Clone)]
pub struct RenameOperation {
    from: String,
    to: String,
}

impl RenameOperation {
    /// Remap the operation whose id is `from` to `to`.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

impl Transform for RenameOperation {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        let mut naming = crate::lifecycle::NamingOverrides::default();
        naming.operations.insert(self.from.clone(), self.to.clone());
        crate::lifecycle::apply_naming(ir, &naming)
    }
}

/// Rename a type (schema) by id-or-bare-name: remap `from` to `to`, rewriting every `$ref` that
/// pointed at it (replaces a `[naming.types]` entry). Reuses [`crate::lifecycle::apply_naming`] so a
/// rename that would collide/collapse/chain is rejected exactly as on the host path.
#[derive(Debug, Clone)]
pub struct RenameType {
    from: String,
    to: String,
}

impl RenameType {
    /// Remap the schema matched by `from` (its id OR bare name) to `to`.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

impl Transform for RenameType {
    fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), CoreError> {
        let mut naming = crate::lifecycle::NamingOverrides::default();
        naming.types.insert(self.from.clone(), self.to.clone());
        crate::lifecycle::apply_naming(ir, &naming)
    }
}

// ---------------------------------------------------------------------------------------------------
// Targets
// ---------------------------------------------------------------------------------------------------

/// The OpenAPI 3.1 target: lowers the frozen IR to an OpenAPI document and writes it at [`OpenApi31::to`].
///
/// Reads `ir.title` / `ir.base_path` / `ir.security` (the metadata transforms set) and calls the
/// existing [`crate::lower::to_openapi`] — NOT a re-implementation. The graph's [`SecurityScheme`]s
/// are passed straight through (`to_openapi` takes `&[SecurityScheme]` directly).
#[derive(Debug, Clone)]
pub struct OpenApi31 {
    path: String,
}

impl OpenApi31 {
    /// An OpenAPI 3.1 target with no output path yet (set with [`OpenApi31::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            path: String::new(),
        }
    }

    /// Set the output path for the OpenAPI document (e.g. `"generated/openapi.yaml"`).
    #[must_use]
    pub fn to(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
}

impl Default for OpenApi31 {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for OpenApi31 {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.path.is_empty() {
            return Err(CoreError::Config {
                message: "OpenApi31 target has no output path — call .to(\"openapi.yaml\")"
                    .to_string(),
            });
        }
        // Pass the graph's security schemes straight to the existing lowering (the single source of
        // truth — an `ApplySecurity` transform set them); never a re-implementation (CLAUDE.md rule 3).
        let doc = crate::lower::to_openapi(ir, &ir.title, &ir.base_path, &ir.security)?;
        out.write(self.path.clone(), doc);
        Ok(())
    }

    /// The OpenAPI artifact path is a loop-safety anchor (a re-run must not ingest the document it
    /// wrote — although it is YAML not Go, declaring it keeps the pipeline's exclusion complete).
    fn output_anchors(&self) -> Vec<String> {
        if self.path.is_empty() {
            Vec::new()
        } else {
            vec![self.path.clone()]
        }
    }
}

/// The Go SDK target: generates the multi-file Go SDK bundle and writes each file under [`GoSdk::to`].
///
/// Derives the SDK's Go package name from [`GoSdk::module`] (the last path segment, sanitized — the
/// same single-source-of-truth derivation the config used), calls the existing
/// [`crate::gosdk::generate`] to produce the bundle, splits it into files via
/// [`crate::gosdk::split_bundle`], and writes each at `<dir>/<name>`.
#[derive(Debug, Clone)]
pub struct GoSdk {
    module: String,
    dir: String,
}

impl GoSdk {
    /// A Go SDK target with no module/output yet (set with [`GoSdk::module`] + [`GoSdk::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            module: String::new(),
            dir: String::new(),
        }
    }

    /// Set the Go module path for the generated SDK (e.g. `"example.com/bookstore/sdk"`). The package
    /// name is derived from this — the single source of truth (CLAUDE.md rule 3).
    #[must_use]
    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.module = module.into();
        self
    }

    /// Set the output directory for the generated SDK files (e.g. `"generated/sdk"`).
    #[must_use]
    pub fn to(mut self, dir: impl Into<String>) -> Self {
        self.dir = dir.into();
        self
    }
}

impl Default for GoSdk {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for GoSdk {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.module.is_empty() {
            return Err(CoreError::Config {
                message: "GoSdk target has no module — call .module(\"example.com/acme/sdk\")"
                    .to_string(),
            });
        }
        if self.dir.is_empty() {
            return Err(CoreError::Config {
                message: "GoSdk target has no output dir — call .to(\"sdk\")".to_string(),
            });
        }
        // Derive the package from the module path (the single source of truth) and generate via the
        // existing deterministic SDK generator — never a re-implementation (CLAUDE.md rules 2 & 3).
        let package = sdk_package(&self.module)?;
        let bundle = crate::gosdk::generate(ir, &package, &ir.base_path)?;
        let dir = self.dir.trim_end_matches('/');
        for (name, contents) in crate::gosdk::split_bundle(&bundle) {
            // Frame names are program-controlled, but reject anything that is not a plain file name so
            // a malformed bundle can never traverse out of `dir` (mirrors gosdk::write_to_dir / the
            // lifecycle write path, T-03-03).
            if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
                return Err(CoreError::SdkGen {
                    message: format!("refusing to emit SDK file with unsafe name {name:?}"),
                });
            }
            out.write(format!("{dir}/{name}"), contents);
        }
        Ok(())
    }

    /// The SDK output directory is the critical loop-safety anchor: the generated `*.go` files form a
    /// Go package inside the analyzed module, so without excluding this dir the source would re-ingest
    /// them and duplicate every schema (the contamination `crate::lifecycle::exclude_output_paths`
    /// prevents on the host path).
    fn output_anchors(&self) -> Vec<String> {
        if self.dir.is_empty() {
            Vec::new()
        } else {
            vec![self.dir.trim_end_matches('/').to_string()]
        }
    }
}

/// The Python SDK target: generates the multi-file Python SDK bundle and writes each file under
/// [`PySdk::to`].
///
/// The structural twin of [`GoSdk`] (minus the `gofmt` step Python has no analog for). Derives the
/// SDK's Python package name from [`PySdk::module`] via the SAME [`sdk_package`] single-source-of-truth
/// derivation `GoSdk` uses (CLAUDE.md rule 3 — no second derivation), takes the URL prefix from
/// `ir.base_path` (the value `SetBasePath` set and the OpenAPI lowering reads — never re-derived),
/// calls the existing [`crate::pysdk::generate`] to produce the bundle, splits it into files via
/// [`crate::pysdk::split_bundle`], and writes each at `<dir>/<name>`.
#[derive(Debug, Clone)]
pub struct PySdk {
    module: String,
    dir: String,
}

impl PySdk {
    /// A Python SDK target with no module/output yet (set with [`PySdk::module`] + [`PySdk::to`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            module: String::new(),
            dir: String::new(),
        }
    }

    /// Set the module path for the generated SDK (e.g. `"example.com/bookstore/sdk"`). The Python
    /// package name is derived from this — the single source of truth (CLAUDE.md rule 3), the same
    /// derivation `GoSdk` uses.
    #[must_use]
    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.module = module.into();
        self
    }

    /// Set the output directory for the generated SDK files (e.g. `"generated/sdk-py"`).
    #[must_use]
    pub fn to(mut self, dir: impl Into<String>) -> Self {
        self.dir = dir.into();
        self
    }
}

impl Default for PySdk {
    fn default() -> Self {
        Self::new()
    }
}

impl Target for PySdk {
    fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        if self.module.is_empty() {
            return Err(CoreError::Config {
                message: "PySdk target has no module — call .module(\"example.com/acme/sdk\")"
                    .to_string(),
            });
        }
        if self.dir.is_empty() {
            return Err(CoreError::Config {
                message: "PySdk target has no output dir — call .to(\"sdk\")".to_string(),
            });
        }
        // Derive the package from the module path via the SAME single source of truth GoSdk uses, and
        // generate via the existing deterministic Python SDK generator — never a re-derivation, never
        // a fallback (CLAUDE.md rules 2 & 3). `ir.base_path` is the same single source of truth the
        // OpenAPI lowering reads (rule 3/4 — never re-derived).
        let package = sdk_package(&self.module)?;
        let bundle = crate::pysdk::generate(ir, &package, &ir.base_path)?;
        let dir = self.dir.trim_end_matches('/');
        for (name, contents) in crate::pysdk::split_bundle(&bundle) {
            // Frame names are program-controlled, but reject anything that is not a plain file name so
            // a malformed bundle can never traverse out of `dir` (mirrors pysdk::write_to_dir / the
            // GoSdk target write path, T-03-02-01).
            if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
                return Err(CoreError::SdkGen {
                    message: format!("refusing to emit SDK file with unsafe name {name:?}"),
                });
            }
            out.write(format!("{dir}/{name}"), contents);
        }
        Ok(())
    }

    /// The SDK output directory is the critical loop-safety anchor: the generated `*.py` files form a
    /// Python package inside the analyzed source tree, so without excluding this dir the source would
    /// re-ingest them and duplicate every schema (the contamination
    /// `crate::lifecycle::exclude_output_paths` prevents on the host path, T-03-02-02).
    fn output_anchors(&self) -> Vec<String> {
        if self.dir.is_empty() {
            Vec::new()
        } else {
            vec![self.dir.trim_end_matches('/').to_string()]
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// PostProcess
// ---------------------------------------------------------------------------------------------------

/// The "Code generated by gnr8" banner line prepended to every generated `.go` file.
const GENERATED_HEADER: &str = "// Code generated by gnr8. DO NOT EDIT.";

/// A post-processor that prepends a "Code generated by gnr8. DO NOT EDIT." line to every `.go`
/// artifact (non-`.go` files are skipped). A small, useful built-in demonstrating the post-process
/// seam; the line is idempotent (a file that already starts with it is left unchanged).
#[derive(Debug, Default, Clone)]
pub struct Header;

impl Header {
    /// The generated-code banner post-processor.
    #[must_use]
    pub fn generated() -> Self {
        Self
    }
}

impl PostProcess for Header {
    fn run(&self, out: &mut Artifacts, _cx: &Cx) -> Result<(), CoreError> {
        // Collect the rewrites first (we can't mutate while iterating `files()`), then re-write each
        // through `Artifacts::write` so the set stays sorted (a rewrite of an existing path replaces
        // it in place). Only `.go` files get the header; the prepend is idempotent.
        let rewrites: Vec<(String, String)> = out
            .files()
            .iter()
            .filter(|a| is_go_file(&a.path))
            .filter(|a| !a.text.starts_with(GENERATED_HEADER))
            .map(|a| (a.path.clone(), format!("{GENERATED_HEADER}\n{}", a.text)))
            .collect();
        for (path, text) in rewrites {
            out.write(path, text);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------------

/// Whether a project-relative artifact `path` is a Go source file (its extension is `go`,
/// case-insensitively) — used to scope the generated-code header to `.go` files only.
fn is_go_file(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("go"))
}

/// Derive the generated SDK's Go package name from a module path — the LAST path segment, sanitized
/// to a valid Go package identifier.
///
/// A single deterministic transform the `GoSdk` target owns: keep ASCII letters/digits lower-cased,
/// drop every separator, trim a leading digit run so the identifier starts with a letter. NOT a
/// fallback — exactly one path; the only branch is input validation.
///
/// # Errors
///
/// Returns [`CoreError::Config`] if `module`'s last segment yields no valid Go identifier (no ASCII
/// letter to anchor it).
fn sdk_package(module: &str) -> Result<String, CoreError> {
    let last = module.rsplit('/').next().unwrap_or("");
    let kept: String = last
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let pkg = kept.trim_start_matches(|c: char| c.is_ascii_digit());
    if pkg.is_empty() {
        return Err(CoreError::Config {
            message: format!(
                "GoSdk module {module:?} has no last path segment that forms a valid Go package \
                 identifier (need at least one ASCII letter, e.g. \"example.com/acme/sdk\")"
            ),
        });
    }
    Ok(pkg.to_string())
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        sdk_package, ApplySecurity, Cx, FastApi, Flask, GoSdk, Header, OpenApi31, PostProcess,
        PySdk, SetBasePath, SetTitle, Source, Target, Transform,
    };
    use crate::graph::ApiGraph;
    use crate::sdk::Artifacts;

    fn cx() -> Cx {
        Cx::new(std::env::temp_dir())
    }

    #[test]
    fn transforms_set_graph_metadata() {
        let mut ir = ApiGraph::default();
        SetBasePath::new("/books").apply(&mut ir, &cx()).unwrap();
        SetTitle::new("Bookstore API")
            .apply(&mut ir, &cx())
            .unwrap();
        ApplySecurity::api_key("ApiKeyAuth", "X-API-Key")
            .apply(&mut ir, &cx())
            .unwrap();
        assert_eq!(ir.base_path, "/books");
        assert_eq!(ir.title, "Bookstore API");
        assert_eq!(ir.security.len(), 1);
        let s = &ir.security[0];
        assert_eq!(s.id, "ApiKeyAuth");
        assert_eq!(s.kind, "apiKey");
        assert_eq!(s.location, "header");
        assert_eq!(s.name, "X-API-Key");
    }

    #[test]
    fn sdk_package_derives_last_segment() {
        assert_eq!(sdk_package("example.com/bookstore/sdk").unwrap(), "sdk");
        assert_eq!(sdk_package("example.com/acme/gnr8sdk").unwrap(), "gnr8sdk");
        assert!(sdk_package("example.com/123").is_err());
    }

    #[test]
    fn targets_error_when_unconfigured() {
        let ir = ApiGraph::default();
        let mut out = Artifacts::new();
        assert!(matches!(
            OpenApi31::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            GoSdk::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            GoSdk::new()
                .module("x.com/sdk")
                .generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            PySdk::new().generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
        assert!(matches!(
            PySdk::new()
                .module("x.com/sdk")
                .generate(&ir, &mut out, &cx()),
            Err(crate::CoreError::Config { .. })
        ));
    }

    #[test]
    fn pysdk_target_writes_under_the_output_dir_and_is_deterministic() {
        let ir = ApiGraph::default();
        let target = PySdk::new()
            .module("example.com/bookstore/sdk")
            .to("generated/sdk-py/");

        // A configured run writes one Artifact per generated Python file, all anchored under the
        // (slash-trimmed) output dir.
        let mut out = Artifacts::new();
        target.generate(&ir, &mut out, &cx()).unwrap();
        assert!(
            !out.files().is_empty(),
            "a configured PySdk run must emit at least one Artifact"
        );
        for artifact in out.files() {
            assert!(
                artifact.path.starts_with("generated/sdk-py/"),
                "every Artifact path must be under the output dir, got {:?}",
                artifact.path
            );
        }

        // The trimmed output dir is the loop-safety anchor (so the pipeline never re-ingests the
        // generated *.py); an unconfigured target anchors nothing.
        assert_eq!(target.output_anchors(), vec!["generated/sdk-py".to_string()]);
        assert!(PySdk::new().output_anchors().is_empty());

        // Two fresh runs over the same IR yield byte-identical Artifacts (T-03-02-05).
        let mut out2 = Artifacts::new();
        target.generate(&ir, &mut out2, &cx()).unwrap();
        let first: Vec<(&str, &str)> = out
            .files()
            .iter()
            .map(|a| (a.path.as_str(), a.text.as_str()))
            .collect();
        let second: Vec<(&str, &str)> = out2
            .files()
            .iter()
            .map(|a| (a.path.as_str(), a.text.as_str()))
            .collect();
        assert_eq!(first, second, "two PySdk runs must be byte-identical");
    }

    #[test]
    fn python_sources_error_when_unconfigured() {
        // Both Python sources reject zero inputs and many inputs with a typed Config error, exactly
        // like GoGin — the single-input guard is identical; only the proper noun differs.
        let cx = cx();
        assert!(
            matches!(
                FastApi::new().load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "FastApi with no inputs must be a Config error"
        );
        assert!(
            matches!(
                FastApi::new().inputs(["a", "b"]).load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "FastApi with many inputs must be a Config error"
        );
        assert!(
            matches!(Flask::new().load(&cx), Err(crate::CoreError::Config { .. })),
            "Flask with no inputs must be a Config error"
        );
        assert!(
            matches!(
                Flask::new().inputs(["a", "b"]).load(&cx),
                Err(crate::CoreError::Config { .. })
            ),
            "Flask with many inputs must be a Config error"
        );
    }

    #[test]
    fn header_prepends_to_go_files_only_and_is_idempotent() {
        let mut out = Artifacts::new();
        out.write("openapi.yaml", "openapi: 3.1.0\n");
        out.write("sdk/client.go", "package sdk\n");
        Header::generated().run(&mut out, &cx()).unwrap();
        let go = out
            .files()
            .iter()
            .find(|f| f.path == "sdk/client.go")
            .unwrap();
        assert!(
            go.text
                .starts_with("// Code generated by gnr8. DO NOT EDIT.\n"),
            "go file gets the header: {:?}",
            go.text
        );
        let yaml = out
            .files()
            .iter()
            .find(|f| f.path == "openapi.yaml")
            .unwrap();
        assert!(
            !yaml.text.contains("Code generated"),
            "non-go file is untouched"
        );
        // Idempotent: running twice does not double the header.
        Header::generated().run(&mut out, &cx()).unwrap();
        let go2 = out
            .files()
            .iter()
            .find(|f| f.path == "sdk/client.go")
            .unwrap();
        assert_eq!(go2.text.matches("Code generated").count(), 1);
    }
}
