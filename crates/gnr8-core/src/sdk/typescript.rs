//! TypeScript SDK surface policies.

use crate::sdk::profile::SdkProfile;

/// How generated TypeScript model/interface properties reflect graph requiredness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TsModelPropertyPolicy {
    /// Preserve the graph's presence facts exactly: optional fields use `?:`, required fields do not.
    #[default]
    Strict,
    /// Match OpenAPI Generator model shape: schema-required fields are required, and fields omitted
    /// from the schema `required` list use `?:`.
    OpenApiRequired,
    /// Emit every generated model property as optional for legacy OpenAPI Generator assignment
    /// compatibility.
    OpenApiGeneratorLoose,
}

impl TsModelPropertyPolicy {
    /// Strict graph-fidelity property declarations.
    #[must_use]
    pub const fn strict() -> Self {
        Self::Strict
    }

    /// Legacy-loose property declarations.
    #[must_use]
    pub const fn openapi_generator_loose() -> Self {
        Self::OpenApiGeneratorLoose
    }

    /// OpenAPI Generator-style property declarations.
    #[must_use]
    pub const fn openapi_required() -> Self {
        Self::OpenApiRequired
    }

    pub(crate) const fn field_optional(self, graph_required: bool, graph_optional: bool) -> bool {
        match self {
            Self::Strict => graph_optional,
            Self::OpenApiRequired => !graph_required,
            Self::OpenApiGeneratorLoose => true,
        }
    }
}

/// TypeScript compatibility profiles exposed as a concise target-level API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsCompatibility {
    /// OpenAPI Generator-compatible TypeScript SDK surface.
    OpenApiGenerator,
}

/// How generated TypeScript model/interface properties represent nullable values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TsNullablePolicy {
    /// Emit `T | null` whenever the graph says the value may be null.
    #[default]
    ExplicitNull,
    /// Emit `T | null` for required nullable properties, but omit `| null` from optional properties.
    OmitNullFromOptionalProperties,
    /// Never emit `| null` in model property types.
    OmitNull,
}

impl TsNullablePolicy {
    /// Preserve graph nullability as explicit TypeScript unions.
    #[must_use]
    pub const fn explicit_null() -> Self {
        Self::ExplicitNull
    }

    /// Treat optional nullable fields as optional-only legacy declarations.
    #[must_use]
    pub const fn omit_null_from_optional_properties() -> Self {
        Self::OmitNullFromOptionalProperties
    }

    /// Omit nullable unions from all model properties.
    #[must_use]
    pub const fn omit_null() -> Self {
        Self::OmitNull
    }

    pub(crate) const fn field_nullable(
        self,
        effective_optional: bool,
        graph_nullable: bool,
    ) -> bool {
        if !graph_nullable {
            return false;
        }
        match self {
            Self::ExplicitNull => true,
            Self::OmitNullFromOptionalProperties => !effective_optional,
            Self::OmitNull => false,
        }
    }
}

/// How TypeScript operation methods expose decoded response data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TsResponsePolicy {
    /// Return decoded response bodies directly as `Promise<T>`.
    #[default]
    DataOnly,
    /// Return Axios response wrappers as `Promise<AxiosResponse<T>>`.
    AxiosResponseWrapper,
}

impl TsResponsePolicy {
    /// Return decoded response bodies directly.
    #[must_use]
    pub const fn data_only() -> Self {
        Self::DataOnly
    }

    /// Return Axios response wrappers.
    #[must_use]
    pub const fn axios_response_wrapper() -> Self {
        Self::AxiosResponseWrapper
    }

    pub(crate) const fn is_axios_response_wrapper(self) -> bool {
        matches!(self, Self::AxiosResponseWrapper)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TsSdkOptions {
    pub(crate) model_properties: TsModelPropertyPolicy,
    pub(crate) nullable: TsNullablePolicy,
    pub(crate) response: TsResponsePolicy,
}

impl TsSdkOptions {
    pub(crate) const fn strict() -> Self {
        Self {
            model_properties: TsModelPropertyPolicy::Strict,
            nullable: TsNullablePolicy::ExplicitNull,
            response: TsResponsePolicy::DataOnly,
        }
    }

    pub(crate) const fn openapi_generator_compat() -> Self {
        Self {
            model_properties: TsModelPropertyPolicy::OpenApiRequired,
            nullable: TsNullablePolicy::ExplicitNull,
            response: TsResponsePolicy::AxiosResponseWrapper,
        }
    }

    pub(crate) fn for_profile(profile: &SdkProfile) -> Self {
        if profile.is_openapi_generator_compat() {
            Self::openapi_generator_compat()
        } else {
            Self::strict()
        }
    }
}

impl Default for TsSdkOptions {
    fn default() -> Self {
        Self::strict()
    }
}
