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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum GoQuerySetterArgumentPolicy {
    /// Use the query parameter's generated Go type.
    #[default]
    Typed,
    /// Accept `any` and serialize through compatibility query helpers.
    Any,
    /// Accept `any` only for selected request-builder setters.
    SelectiveAny(BTreeMap<String, BTreeSet<String>>),
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

    /// Widen one generated query setter on one request builder to `any`.
    #[must_use]
    pub fn any_for(mut self, request: impl Into<String>, setter: impl Into<String>) -> Self {
        match &mut self {
            Self::Any => self,
            Self::Typed => {
                let mut any_for = BTreeMap::new();
                any_for
                    .entry(request.into())
                    .or_insert_with(BTreeSet::new)
                    .insert(setter.into());
                Self::SelectiveAny(any_for)
            }
            Self::SelectiveAny(any_for) => {
                any_for
                    .entry(request.into())
                    .or_insert_with(BTreeSet::new)
                    .insert(setter.into());
                self
            }
        }
    }

    /// Widen query setters matching a query parameter name to `any` on every request builder.
    #[must_use]
    pub fn any_for_query(self, query_name: impl Into<String>) -> Self {
        self.any_for_operation_query("*", query_name)
    }

    /// Widen query setters matching any of the query parameter names to `any`.
    #[must_use]
    pub fn any_for_queries<I, S>(mut self, query_names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for query_name in query_names {
            self = self.any_for_query(query_name);
        }
        self
    }

    /// Widen a query setter matching a query parameter name on one operation id.
    #[must_use]
    pub fn any_for_operation_query(
        mut self,
        operation: impl Into<String>,
        query_name: impl Into<String>,
    ) -> Self {
        let request = format!("operation:{}", operation.into());
        let setter = format!("query:{}", query_name.into());
        self = self.any_for(request, setter);
        self
    }

    /// Widen a query setter matching a query parameter name on one method/path route.
    #[must_use]
    pub fn any_for_route_query(
        self,
        method: impl Into<String>,
        path: impl Into<String>,
        query_name: impl Into<String>,
    ) -> Self {
        let request = format!(
            "route:{} {}",
            method.into().to_ascii_uppercase(),
            path.into()
        );
        let setter = format!("query:{}", query_name.into());
        self.any_for(request, setter)
    }

    pub(crate) fn is_any_for(&self, request: &str, setter: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Typed => false,
            Self::SelectiveAny(any_for) => any_for
                .get(request)
                .is_some_and(|setters| setters.contains(setter)),
        }
    }

    pub(crate) fn request_names(&self) -> BTreeSet<String> {
        match self {
            Self::SelectiveAny(any_for) => any_for.keys().cloned().collect(),
            Self::Typed | Self::Any => BTreeSet::new(),
        }
    }

    pub(crate) fn setters_for(&self, request: &str) -> BTreeSet<String> {
        match self {
            Self::SelectiveAny(any_for) => any_for.get(request).cloned().unwrap_or_default(),
            Self::Typed | Self::Any => BTreeSet::new(),
        }
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

    /// Add aliases to the request builder for one method/path route.
    #[must_use]
    pub fn operation(
        self,
        method: impl Into<String>,
        path: impl Into<String>,
    ) -> GoRequestBuilderOperationAliases {
        GoRequestBuilderOperationAliases {
            aliases: self,
            selector: format!(
                "route:{} {}",
                method.into().to_ascii_uppercase(),
                path.into()
            ),
        }
    }

    /// Add aliases to the request builder for one operation id.
    #[must_use]
    pub fn operation_id(self, operation: impl Into<String>) -> GoRequestBuilderOperationAliases {
        GoRequestBuilderOperationAliases {
            aliases: self,
            selector: format!("operation:{}", operation.into()),
        }
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

/// Builder returned by [`GoRequestBuilderAliases::operation`] and
/// [`GoRequestBuilderAliases::operation_id`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoRequestBuilderOperationAliases {
    aliases: GoRequestBuilderAliases,
    selector: String,
}

impl GoRequestBuilderOperationAliases {
    /// Add an alias body setter for the selected operation request builder.
    #[must_use]
    pub fn body(self, setter: impl Into<String>) -> GoRequestBuilderAliases {
        self.aliases.body(self.selector, setter)
    }

    /// Add an alias query setter for the selected operation request builder.
    #[must_use]
    pub fn query(
        self,
        setter: impl Into<String>,
        query_name: impl Into<String>,
    ) -> GoRequestBuilderAliases {
        self.aliases.query(self.selector, setter, query_name)
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

    /// Preserve the legacy `Execute` signature for an operation matched by method/path.
    #[must_use]
    pub fn route(mut self, method: impl Into<String>, path: impl Into<String>) -> Self {
        self.preserve_legacy_operations.insert(format!(
            "route:{} {}",
            method.into().to_ascii_uppercase(),
            path.into()
        ));
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
