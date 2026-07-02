//! Go SDK surface policies.

use crate::sdk::profile::SdkProfile;

/// How OpenAPI Generator-compatible model constructors accept required pointer fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequiredPointerConstructorPolicy {
    /// Preserve pointer parameters for required pointer fields.
    #[default]
    PointerParam,
    /// Accept value parameters and assign their addresses to required pointer fields.
    ValueParam,
}

impl RequiredPointerConstructorPolicy {
    /// Preserve pointer parameters.
    #[must_use]
    pub const fn pointer_param() -> Self {
        Self::PointerParam
    }

    /// Accept value parameters for required pointer fields.
    #[must_use]
    pub const fn value_param() -> Self {
        Self::ValueParam
    }
}

/// How `time.Time` query values are serialized in OpenAPI Generator-compatible helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryTimeFormat {
    /// Use the default `fmt.Sprint` behavior.
    #[default]
    Default,
    /// Format midnight values as `YYYY-MM-DD`; format other values as RFC3339.
    DateOnlyAtMidnightElseRfc3339,
}

impl QueryTimeFormat {
    /// Preserve the default Go formatting behavior.
    #[must_use]
    pub const fn default_format() -> Self {
        Self::Default
    }

    /// Format midnight values as dates and all other values as RFC3339 timestamps.
    #[must_use]
    pub const fn date_only_at_midnight_else_rfc3339() -> Self {
        Self::DateOnlyAtMidnightElseRfc3339
    }

    pub(crate) const fn needs_time_import(self) -> bool {
        matches!(self, Self::DateOnlyAtMidnightElseRfc3339)
    }
}

/// Which setters are emitted on OpenAPI Generator-compatible request builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GoRequestBuilderScope {
    /// Emit only setters used by the operation.
    #[default]
    Operation,
    /// Emit legacy graph-wide compatibility setters on every request builder.
    Global,
}

impl GoRequestBuilderScope {
    /// Emit only operation-local request builder setters.
    #[must_use]
    pub const fn operation() -> Self {
        Self::Operation
    }

    /// Emit legacy graph-wide request builder setters.
    #[must_use]
    pub const fn global() -> Self {
        Self::Global
    }

    pub(crate) const fn is_global(self) -> bool {
        matches!(self, Self::Global)
    }
}

/// Go SDK compatibility options exposed by [`crate::sdk::builtins::GoSdk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoSdkOptions {
    pub(crate) error_model: Option<String>,
    pub(crate) required_pointer_constructor_policy: RequiredPointerConstructorPolicy,
    pub(crate) query_time_format: QueryTimeFormat,
    pub(crate) request_builder_scope: GoRequestBuilderScope,
}

impl GoSdkOptions {
    pub(crate) fn strict() -> Self {
        Self {
            error_model: None,
            required_pointer_constructor_policy: RequiredPointerConstructorPolicy::PointerParam,
            query_time_format: QueryTimeFormat::Default,
            request_builder_scope: GoRequestBuilderScope::Operation,
        }
    }

    pub(crate) fn for_profile(_profile: &SdkProfile) -> Self {
        Self::strict()
    }
}

impl Default for GoSdkOptions {
    fn default() -> Self {
        Self::strict()
    }
}
