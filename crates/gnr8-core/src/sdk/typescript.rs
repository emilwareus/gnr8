//! TypeScript SDK surface policies.

use crate::sdk::profile::SdkProfile;

/// How generated TypeScript model/interface properties reflect graph requiredness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TsModelPropertyPolicy {
    /// Preserve the graph's presence facts exactly: optional fields use `?:`, required fields do not.
    #[default]
    Strict,
    /// Derive optional markers from the OpenAPI schema `required` list.
    SchemaRequired,
    /// Emit every generated model property as optional.
    AllOptional,
}

impl TsModelPropertyPolicy {
    /// Strict graph-fidelity property declarations.
    #[must_use]
    pub const fn strict() -> Self {
        Self::Strict
    }

    /// Legacy-loose property declarations.
    #[must_use]
    pub const fn all_optional() -> Self {
        Self::AllOptional
    }

    /// OpenAPI schema-required property declarations.
    #[must_use]
    pub const fn openapi_required() -> Self {
        Self::SchemaRequired
    }

    pub(crate) const fn field_optional(self, graph_required: bool, graph_optional: bool) -> bool {
        match self {
            Self::Strict => graph_optional,
            Self::SchemaRequired => !graph_required,
            Self::AllOptional => true,
        }
    }
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
}

impl TsResponsePolicy {
    /// Return decoded response bodies directly.
    #[must_use]
    pub const fn data_only() -> Self {
        Self::DataOnly
    }
}

/// How the TypeScript SDK emits the root `index.ts` barrel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TsBarrelExports {
    /// Re-export every API, runtime, and model symbol.
    #[default]
    Star,
}

impl TsBarrelExports {
    /// Re-export every generated symbol.
    #[must_use]
    pub const fn star() -> Self {
        Self::Star
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TsSdkOptions {
    pub(crate) model_properties: TsModelPropertyPolicy,
    pub(crate) nullable: TsNullablePolicy,
    pub(crate) response: TsResponsePolicy,
    pub(crate) request_body_param_name: String,
    pub(crate) init_override_function: bool,
    pub(crate) barrel_exports: TsBarrelExports,
}

impl TsSdkOptions {
    pub(crate) fn strict() -> Self {
        Self {
            model_properties: TsModelPropertyPolicy::Strict,
            nullable: TsNullablePolicy::ExplicitNull,
            response: TsResponsePolicy::DataOnly,
            request_body_param_name: "body".to_string(),
            init_override_function: false,
            barrel_exports: TsBarrelExports::Star,
        }
    }

    pub(crate) fn for_profile(_profile: &SdkProfile) -> Self {
        Self::strict()
    }
}

impl Default for TsSdkOptions {
    fn default() -> Self {
        Self::strict()
    }
}
