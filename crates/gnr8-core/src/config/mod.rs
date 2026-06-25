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
//! - `base_path` — the API base/mount path (see below),
//! - `output.openapi` / `output.sdk_dir` / `output.go_module` — artifact paths + Go module path,
//! - `naming.operations` / `naming.types` — operation/type name remaps,
//! - `security.schemes` — the API security schemes (see below).
//!
//! ## Security is config, never scraped (CLAUDE.md rules 1 & 4)
//!
//! Security schemes (API keys, etc.) live in the auth middleware, not in handler signatures or typed
//! source — so the engine genuinely cannot derive them from code. They are therefore provided HERE, by
//! the user configuring our engine, and are the **single source of truth** for the generated
//! `security` requirement + `components.securitySchemes`. We never scrape them from another tool's
//! annotations. The PoC policy is intentionally minimal: every listed scheme applies to ALL operations
//! (a per-operation policy is a documented v2 direction).
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
    /// The API base/mount path joined to every group-relative operation path — the **single source
    /// of truth** for the service prefix (CLAUDE.md rules 3 & 4).
    ///
    /// The graph stores group-relative paths (`/`, `/list`, `/{uuid}`) and carries no service prefix:
    /// the real prefix is the Gin group argument (`Group("/" + basePath)`), often a *runtime* value
    /// the analyzer cannot constant-fold. Rather than scrape or guess it (and rather than read a
    /// literal with a config fall-back — forbidden by rule 3), it is declared HERE, by the user
    /// configuring our engine, and is the only place lowering takes it from. Defaults to `"/"` (a
    /// root-mounted service) when omitted.
    #[serde(default = "default_base_path")]
    pub base_path: String,
    /// The OpenAPI document title (`info.title`) — API-author metadata the typed Go source does not
    /// carry, so it is declared HERE by the user configuring our engine (CLAUDE.md rule 4), the single
    /// place lowering takes it from. Defaults to `"API"` when omitted.
    #[serde(default = "default_title")]
    pub title: String,
    /// Output artifact paths + Go module path for the generated SDK.
    pub output: OutputConfig,
    /// Optional operation/type name remaps (the one customization knob built in the PoC).
    #[serde(default)]
    pub naming: NamingOverrides,
    /// The API security configuration — the single source of truth for the generated `security`
    /// requirement and `components.securitySchemes`. Defaults to no schemes when omitted.
    #[serde(default)]
    pub security: SecurityConfig,
}

/// The default API base path (`"/"`, a root-mounted service) used when `base_path` is omitted.
///
/// A function rather than a const because `#[serde(default = "...")]` requires a callable producing
/// the owned `String`.
fn default_base_path() -> String {
    "/".to_string()
}

/// The default OpenAPI title (`"API"`) used when `title` is omitted.
///
/// A function rather than a const because `#[serde(default = "...")]` requires a callable producing
/// the owned `String`.
fn default_title() -> String {
    "API".to_string()
}

/// The security configuration: the user-declared schemes that secure the generated API.
///
/// This is the code-as-config home for security (CLAUDE.md rule 4): the engine cannot derive auth
/// from typed source, so the user declares it here and it is the ONLY source. The PoC policy applies
/// every listed scheme to ALL operations (`apply_to_all`); a per-operation policy is a v2 direction.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityConfig {
    /// The declared security schemes, keyed by their OpenAPI scheme id (e.g. `ApiKeyAuth`). Each
    /// scheme is emitted into `components.securitySchemes`; under the PoC `apply_to_all` policy each
    /// is also added to the top-level `security` requirement.
    #[serde(default)]
    pub schemes: Vec<SecurityScheme>,
}

/// One declared security scheme. The PoC supports the `apiKey`-in-`header` shape the fixture uses;
/// `kind`/`location` are validated at lowering time so an unsupported combination is a clear error
/// rather than a silently dropped scheme.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityScheme {
    /// The OpenAPI scheme id (the key under `components.securitySchemes`, e.g. `"ApiKeyAuth"`).
    pub id: String,
    /// The scheme kind. The PoC supports `"apiKey"`.
    pub kind: String,
    /// Where the credential is read from. The PoC supports `"header"`.
    pub location: String,
    /// The credential name (for an `apiKey`/`header` scheme, the header name, e.g. `"X-API-Key"`).
    pub name: String,
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

impl OutputConfig {
    /// Derive the generated SDK's Go package name from `go_module` — the **single source of truth**.
    ///
    /// The package name is a pure, deterministic transform of one config value (CLAUDE.md rule 3): the
    /// LAST path segment of `go_module`, sanitized to a valid Go package identifier. Sanitization keeps
    /// ASCII letters/digits and lower-cases them, dropping every separator (`-`, `.`, `/`, `_`, …); a
    /// leading non-letter is then trimmed so the identifier always starts with a letter. Examples:
    /// `example.com/bookstore/sdk` → `sdk`; `example.com/acme/gnr8sdk` → `gnr8sdk`;
    /// `github.com/acme/svc/goalservice` → `goalservice`.
    ///
    /// This is NOT a fallback: there is exactly one path. The only branch is input validation of that
    /// single source — a `go_module` whose last segment yields no valid identifier (empty, or only
    /// separators/digits) is a typed [`CoreError::Config`] rather than a silent second source.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] if `go_module`'s last segment cannot form a valid Go package
    /// identifier (no ASCII letter to anchor it).
    pub fn sdk_package(&self) -> Result<String, CoreError> {
        let last = self.go_module.rsplit('/').next().unwrap_or("");
        // Keep ASCII alphanumerics (lower-cased); drop every separator. A Go package identifier must
        // begin with a letter, so trim any leading run of digits that survives.
        let kept: String = last
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        let pkg = kept.trim_start_matches(|c: char| c.is_ascii_digit());
        if pkg.is_empty() {
            return Err(CoreError::Config {
                message: format!(
                    "output.go_module {:?} has no last path segment that forms a valid Go package \
                     identifier (need at least one ASCII letter, e.g. \"example.com/acme/sdk\")",
                    self.go_module
                ),
            });
        }
        Ok(pkg.to_string())
    }
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

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow to
    // the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::parse;

    const BASE: &str = "inputs = [\".\"]\n\n[output]\nopenapi = \"openapi.yaml\"\nsdk_dir = \"sdk\"\ngo_module = \"example.com/svc/sdk\"\n";

    #[test]
    fn base_path_defaults_to_root_when_omitted() {
        let config = parse(BASE).unwrap();
        assert_eq!(
            config.base_path, "/",
            "base_path must default to \"/\" (a root-mounted service) when omitted"
        );
    }

    #[test]
    fn parses_an_explicit_base_path() {
        // base_path is a top-level key, so it must precede the [output] table.
        let src = "inputs = [\".\"]\nbase_path = \"/books\"\n\n[output]\nopenapi = \"openapi.yaml\"\nsdk_dir = \"sdk\"\ngo_module = \"example.com/svc/sdk\"\n";
        let config = parse(src).unwrap();
        assert_eq!(config.base_path, "/books");
    }

    #[test]
    fn security_defaults_to_no_schemes_when_omitted() {
        let config = parse(BASE).unwrap();
        assert!(
            config.security.schemes.is_empty(),
            "security must default to an empty scheme list when [security] is omitted"
        );
    }

    #[test]
    fn parses_an_api_key_security_scheme() {
        let src = format!(
            "{BASE}\n[[security.schemes]]\nid = \"ApiKeyAuth\"\nkind = \"apiKey\"\nlocation = \"header\"\nname = \"X-API-Key\"\n"
        );
        let config = parse(&src).unwrap();
        assert_eq!(config.security.schemes.len(), 1);
        let scheme = &config.security.schemes[0];
        assert_eq!(scheme.id, "ApiKeyAuth");
        assert_eq!(scheme.kind, "apiKey");
        assert_eq!(scheme.location, "header");
        assert_eq!(scheme.name, "X-API-Key");
    }

    #[test]
    fn rejects_unknown_security_field() {
        // deny_unknown_fields must reject a typo'd security scheme key.
        let src = format!(
            "{BASE}\n[[security.schemes]]\nid = \"ApiKeyAuth\"\nkind = \"apiKey\"\nlocation = \"header\"\nname = \"X-API-Key\"\nunexpected = true\n"
        );
        assert!(
            parse(&src).is_err(),
            "an unknown security-scheme key must be rejected by deny_unknown_fields"
        );
    }

    /// Build an `OutputConfig` whose `go_module` is `module`, for `sdk_package` derivation tests.
    fn output_with_module(module: &str) -> super::OutputConfig {
        let src = format!(
            "inputs = [\".\"]\n\n[output]\nopenapi = \"openapi.yaml\"\nsdk_dir = \"sdk\"\ngo_module = {module:?}\n"
        );
        parse(&src).unwrap().output
    }

    #[test]
    fn sdk_package_is_the_sanitized_last_segment_of_go_module() {
        // The package is the LAST path segment, lower-cased, with separators dropped.
        assert_eq!(
            output_with_module("example.com/bookstore/sdk")
                .sdk_package()
                .unwrap(),
            "sdk"
        );
        assert_eq!(
            output_with_module("example.com/acme/gnr8sdk")
                .sdk_package()
                .unwrap(),
            "gnr8sdk"
        );
        assert_eq!(
            output_with_module("github.com/acme/svc/goalservice")
                .sdk_package()
                .unwrap(),
            "goalservice"
        );
        // A single (unslashed) segment is itself the package.
        assert_eq!(output_with_module("mysdk").sdk_package().unwrap(), "mysdk");
        // Separators inside the last segment are dropped, and casing is normalized.
        assert_eq!(
            output_with_module("example.com/My-API.v2")
                .sdk_package()
                .unwrap(),
            "myapiv2"
        );
        // A leading digit run is trimmed so the identifier starts with a letter.
        assert_eq!(
            output_with_module("example.com/2sdk")
                .sdk_package()
                .unwrap(),
            "sdk"
        );
    }

    #[test]
    fn sdk_package_rejects_a_segment_with_no_letter() {
        // Input validation of the single source (not a fallback): a last segment that yields no valid
        // Go identifier (empty / only separators / only digits) is a typed Config error.
        for module in [
            "example.com/sdk/", // trailing slash → empty last segment
            "example.com/__",   // only separators
            "example.com/123",  // only digits → no letter anchor
            "",                 // empty go_module
        ] {
            assert!(
                output_with_module(module).sdk_package().is_err(),
                "go_module {module:?} must be rejected (no valid package identifier)"
            );
        }
    }
}
