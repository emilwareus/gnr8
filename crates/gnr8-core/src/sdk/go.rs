//! Go SDK surface policies.

use std::collections::{BTreeMap, BTreeSet};

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

/// How OpenAPI Generator-compatible query setter arguments are typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GoQuerySetterArgumentPolicy {
    /// Use the query parameter's generated Go type.
    #[default]
    Typed,
    /// Accept `any` and serialize through compatibility query helpers.
    Any,
}

impl GoQuerySetterArgumentPolicy {
    /// Use typed query setter arguments.
    #[must_use]
    pub const fn typed() -> Self {
        Self::Typed
    }

    /// Accept `any` query setter arguments.
    #[must_use]
    pub const fn any() -> Self {
        Self::Any
    }

    pub(crate) const fn is_any(self) -> bool {
        matches!(self, Self::Any)
    }
}

/// Additional methods to emit on OpenAPI Generator-compatible request builders.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GoRequestBuilderAliases {
    pub(crate) body: BTreeMap<String, BTreeSet<String>>,
    pub(crate) query: BTreeMap<String, BTreeSet<GoRequestBuilderQueryAlias>>,
}

/// A query setter alias for a single request builder.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GoRequestBuilderQueryAlias {
    pub(crate) setter: String,
    pub(crate) query_name: String,
}

impl GoRequestBuilderAliases {
    /// Create an empty request-builder alias set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an alias body setter to a request builder type.
    #[must_use]
    pub fn body(mut self, request: impl Into<String>, setter: impl Into<String>) -> Self {
        self.body
            .entry(request.into())
            .or_default()
            .insert(setter.into());
        self
    }

    /// Add an alias query setter to a request builder type.
    #[must_use]
    pub fn query(
        mut self,
        request: impl Into<String>,
        setter: impl Into<String>,
        query_name: impl Into<String>,
    ) -> Self {
        self.query
            .entry(request.into())
            .or_default()
            .insert(GoRequestBuilderQueryAlias {
                setter: setter.into(),
                query_name: query_name.into(),
            });
        self
    }

    pub(crate) fn body_aliases_for(&self, request: &str) -> Vec<String> {
        self.body
            .get(request)
            .map(|aliases| aliases.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn query_aliases_for(&self, request: &str) -> Vec<GoRequestBuilderQueryAlias> {
        self.query
            .get(request)
            .map(|aliases| aliases.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub(crate) fn request_names(&self) -> BTreeSet<String> {
        self.body.keys().chain(self.query.keys()).cloned().collect()
    }
}

/// Compatibility wrappers for OpenAPI Generator-compatible `Execute` methods.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GoExecuteCompatibility {
    preserve_legacy_requests: BTreeSet<String>,
    preserve_legacy_operations: BTreeSet<String>,
}

impl GoExecuteCompatibility {
    /// Preserve selected old `Execute() (*http.Response, error)` signatures.
    #[must_use]
    pub fn preserve_legacy() -> Self {
        Self::default()
    }

    /// Preserve the legacy `Execute` signature for a request builder type.
    #[must_use]
    pub fn request(mut self, request: impl Into<String>) -> Self {
        self.preserve_legacy_requests.insert(request.into());
        self
    }

    /// Preserve the legacy `Execute` signature for an operation id.
    #[must_use]
    pub fn operation(mut self, operation: impl Into<String>) -> Self {
        self.preserve_legacy_operations.insert(operation.into());
        self
    }

    pub(crate) fn preserves(&self, request: &str, operation: &str) -> bool {
        self.preserve_legacy_requests.contains(request)
            || self.preserve_legacy_operations.contains(operation)
    }

    pub(crate) fn request_names(&self) -> &BTreeSet<String> {
        &self.preserve_legacy_requests
    }

    pub(crate) fn operation_names(&self) -> &BTreeSet<String> {
        &self.preserve_legacy_operations
    }
}

/// Go SDK compatibility options exposed by [`crate::sdk::builtins::GoSdk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GoSdkOptions {
    pub(crate) error_model: Option<String>,
    pub(crate) required_pointer_constructor_policy: RequiredPointerConstructorPolicy,
    pub(crate) query_time_format: QueryTimeFormat,
    pub(crate) request_builder_scope: GoRequestBuilderScope,
    pub(crate) request_builder_aliases: GoRequestBuilderAliases,
    pub(crate) query_setter_argument_policy: GoQuerySetterArgumentPolicy,
    pub(crate) execute_compatibility: GoExecuteCompatibility,
}

impl GoSdkOptions {
    pub(crate) fn strict() -> Self {
        Self {
            error_model: None,
            required_pointer_constructor_policy: RequiredPointerConstructorPolicy::PointerParam,
            query_time_format: QueryTimeFormat::Default,
            request_builder_scope: GoRequestBuilderScope::Operation,
            request_builder_aliases: GoRequestBuilderAliases::default(),
            query_setter_argument_policy: GoQuerySetterArgumentPolicy::Typed,
            execute_compatibility: GoExecuteCompatibility::default(),
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
