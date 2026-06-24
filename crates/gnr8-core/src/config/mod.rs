//! The checked-in `.gnr8/config.toml` surface, parsed statically into a typed [`Config`]
//! (WS-03, D-03).
//!
//! **This TOML file is an explicit PoC code-as-config STAND-IN, NOT the long-term UX.** PROJECT
//! forbids a dynamic plugin runtime and scopes "YAML/TOML/JSON" away from the main customization
//! surface; the long-term, programmatic ("through code") customization of routing recognition /
//! transport / emitters is a documented v2 direction (REQUIREMENTS ADV-02). To stay honest, this
//! module models ONLY the documented knobs the PoC actually reads statically:
//!
//! - `inputs` — Go source dir(s) to analyze,
//! - `output.openapi` / `output.sdk_dir` / `output.go_module` — artifact paths + Go module path,
//! - `naming.operations` / `naming.types` — operation/type name remaps.
//!
//! There is deliberately **no** plugin/seam field here: adding an empty stub would overclaim a
//! capability that does not exist. The v2 extension seam is documented in prose, not faked in the
//! type. `#[serde(deny_unknown_fields)]` rejects typo'd/unsupported keys with a clear typed error
//! (`CoreError::Config`) instead of silently mis-generating (V5 input validation, T-04-01-02/03).

// These docs are user-facing prose dense with proper nouns/acronyms (PoC, OpenAPI, TOML, JSON,
// BTreeMap, ...); backticking them would hurt readability. Allow `doc_markdown` module-wide
// (skill ch.2.4, mirrors the scoped allow in gnr8/src/cli.rs).
#![allow(clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::path::Path;

use crate::CoreError;

/// The typed `.gnr8/config.toml` surface — the documented PoC knobs ONLY (D-03).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Go source input directory(ies) to analyze, relative to the project root (e.g. `["."]`).
    pub inputs: Vec<String>,
    /// Output artifact paths + Go module path for the generated SDK.
    pub output: OutputConfig,
    /// Optional operation/type name remaps (the one customization knob built in the PoC).
    #[serde(default)]
    pub naming: NamingOverrides,
}

/// Where generated artifacts are written + the Go module path for the SDK (project-relative).
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// OpenAPI artifact path, e.g. `"openapi.yaml"`.
    pub openapi: String,
    /// Generated Go SDK directory, e.g. `"sdk"`.
    pub sdk_dir: String,
    /// Go module path for the generated SDK, e.g. `"github.com/acme/svc/sdk"`.
    pub go_module: String,
}

/// Optional name remaps applied to generated operations/types. `BTreeMap` keeps the maps sorted so
/// downstream use stays deterministic (mirrors the graph's sorted-collection policy, GRAPH-02).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamingOverrides {
    /// Operation-id remaps, e.g. `goalUuidPut = "UpdateGoal"`.
    #[serde(default)]
    pub operations: BTreeMap<String, String>,
    /// Generated type-name remaps, e.g. `CreateGoalInput = "NewGoal"`.
    #[serde(default)]
    pub types: BTreeMap<String, String>,
}

/// Parse a `config.toml` source string into a typed [`Config`].
///
/// # Errors
///
/// Returns [`CoreError::Config`] for malformed TOML or an unknown/typo'd key (rejected by
/// `deny_unknown_fields`). Never panics (RUST-04 / T-04-01-02, T-04-01-03).
pub fn parse(toml_src: &str) -> Result<Config, CoreError> {
    toml::from_str(toml_src).map_err(|e| CoreError::Config {
        message: e.to_string(),
    })
}

/// Read and parse `<gnr8_dir>/config.toml` into a typed [`Config`].
///
/// # Errors
///
/// Returns [`CoreError::Config`] if the file is missing/unreadable (with an actionable message
/// pointing the user at `gnr8 init`) or if its contents fail to parse. Never panics.
pub fn load(gnr8_dir: &Path) -> Result<Config, CoreError> {
    let path = gnr8_dir.join("config.toml");
    let src = std::fs::read_to_string(&path).map_err(|e| CoreError::Config {
        message: format!(
            "failed to read {} ({e}); run `gnr8 init` to scaffold the workspace",
            path.display()
        ),
    })?;
    parse(&src)
}
