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
    OpenApiGeneratorCompat,
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
        Self {
            kind: SdkProfileKind::OpenApiGeneratorCompat,
        }
    }

    /// Whether this is the minimal profile.
    #[must_use]
    pub(crate) const fn is_minimal(&self) -> bool {
        matches!(self.kind, SdkProfileKind::Minimal)
    }

    /// Whether this profile requests OpenAPI-generator compatibility.
    #[must_use]
    pub(crate) const fn is_openapi_generator_compat(&self) -> bool {
        matches!(self.kind, SdkProfileKind::OpenApiGeneratorCompat)
    }

    /// Stable machine-readable profile name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self.kind {
            SdkProfileKind::Minimal => "minimal",
            SdkProfileKind::OpenApiGeneratorCompat => "openapi_generator_compat",
        }
    }
}

impl Default for SdkProfile {
    fn default() -> Self {
        Self::minimal()
    }
}
