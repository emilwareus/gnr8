//! SDK generation profiles.
//!
//! Profiles are target-surface policy, not source facts. `minimal()` preserves the historical gnr8
//! SDKs; compatibility profiles can add runtime files and aliases without changing the graph.

/// SDK generation profile shared by built-in targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkProfile {
    kind: SdkProfileKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SdkProfileKind {
    Minimal,
    TypeScriptFetchCompat,
    TypeScriptAxiosCompat,
    GoOpenApiGeneratorCompat,
}

impl SdkProfile {
    /// Preserve the historical minimal gnr8 SDK surface.
    #[must_use]
    pub fn minimal() -> Self {
        Self {
            kind: SdkProfileKind::Minimal,
        }
    }

    /// Emit an OpenAPI-generator-compatible SDK surface where supported.
    #[must_use]
    pub fn openapi_generator_compat() -> Self {
        Self::typescript_axios_compat()
    }

    /// Emit a TypeScript `fetch` SDK surface with OpenAPI Generator-compatible model policies.
    #[must_use]
    pub fn typescript_fetch_compat() -> Self {
        Self {
            kind: SdkProfileKind::TypeScriptFetchCompat,
        }
    }

    /// Emit a TypeScript `axios` SDK surface compatible with OpenAPI Generator's TypeScript output.
    #[must_use]
    pub fn typescript_axios_compat() -> Self {
        Self {
            kind: SdkProfileKind::TypeScriptAxiosCompat,
        }
    }

    /// Emit a Go SDK surface compatible with OpenAPI Generator's Go client shape.
    #[must_use]
    pub fn go_openapi_generator_compat() -> Self {
        Self {
            kind: SdkProfileKind::GoOpenApiGeneratorCompat,
        }
    }

    /// Whether this is the minimal profile.
    #[must_use]
    pub(crate) const fn is_minimal(&self) -> bool {
        matches!(self.kind, SdkProfileKind::Minimal)
    }

    /// Whether this profile requests TypeScript fetch compatibility.
    #[must_use]
    pub(crate) const fn is_typescript_fetch_compat(&self) -> bool {
        matches!(self.kind, SdkProfileKind::TypeScriptFetchCompat)
    }

    /// Whether this profile requests TypeScript axios compatibility.
    #[must_use]
    pub(crate) const fn is_typescript_axios_compat(&self) -> bool {
        matches!(self.kind, SdkProfileKind::TypeScriptAxiosCompat)
    }

    /// Whether this profile requests OpenAPI Generator-compatible Go output.
    #[must_use]
    pub(crate) const fn is_go_openapi_generator_compat(&self) -> bool {
        matches!(self.kind, SdkProfileKind::GoOpenApiGeneratorCompat)
    }

    /// Stable machine-readable profile name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self.kind {
            SdkProfileKind::Minimal => "minimal",
            SdkProfileKind::TypeScriptFetchCompat => "typescript_fetch_compat",
            SdkProfileKind::TypeScriptAxiosCompat => "typescript_axios_compat",
            SdkProfileKind::GoOpenApiGeneratorCompat => "go_openapi_generator_compat",
        }
    }
}

impl Default for SdkProfile {
    fn default() -> Self {
        Self::minimal()
    }
}
