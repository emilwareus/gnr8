//! Public SDK surface diffing helpers.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::CoreError;

/// Type/value namespace kind for an exported TypeScript symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TsExportKind {
    /// Type-only export.
    Type,
    /// Runtime value export.
    Value,
    /// Exported in both namespaces.
    Both,
}

/// Extracted TypeScript public surface.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct TypeScriptSurface {
    /// Root exports reachable from `index.ts`.
    pub root_exports: BTreeMap<String, TsExportKind>,
    /// Model exports.
    pub model_exports: BTreeMap<String, TsExportKind>,
    /// Exported API class names.
    pub api_classes: Vec<String>,
    /// Exported API factory names.
    pub api_factories: Vec<String>,
    /// Exported API class operation methods as `Class.method`.
    pub operation_methods: Vec<String>,
    /// Request alias names.
    pub request_aliases: Vec<String>,
    /// Exported interface properties keyed by interface name, then property name.
    pub interface_properties: BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
    /// Canonical public type declarations, including aliases, heritage, enums, and enum-like values.
    pub type_declarations: BTreeMap<String, Vec<String>>,
    /// Operation/factory return types keyed by `Class.method` or `Factory.method`.
    pub operation_return_types: BTreeMap<String, String>,
    /// Operation/factory method signatures keyed by `Class.method` or `Factory.method`.
    pub operation_signatures: BTreeMap<String, String>,
    /// Package entry point fields.
    pub package_entry_points: BTreeMap<String, String>,
    /// Documentation files present in the SDK package.
    pub docs: Vec<String>,
}

/// TypeScript surface diff report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct TypeScriptSurfaceDiff {
    /// Root exports present in old but missing in new.
    pub missing_root_exports: Vec<String>,
    /// Model exports present in old but missing in new.
    pub missing_model_exports: Vec<String>,
    /// Exports whose type/value namespace changed.
    pub export_kind_mismatches: Vec<TsExportKindMismatch>,
    /// API classes present in old but missing in new.
    pub missing_api_classes: Vec<String>,
    /// API factories present in old but missing in new.
    pub missing_api_factories: Vec<String>,
    /// Operation methods present in old but missing in new.
    pub missing_operation_methods: Vec<String>,
    /// Request aliases present in old but missing in new.
    pub missing_request_aliases: Vec<String>,
    /// Interface properties present in old but missing in new.
    pub missing_interface_properties: Vec<TsMissingInterfaceProperty>,
    /// Interface properties whose optionality, nullability, or type changed.
    pub interface_property_changes: Vec<TsInterfacePropertyChange>,
    /// Interface properties that changed from required to optional.
    pub interface_required_to_optional: Vec<TsInterfacePropertyChange>,
    /// Interface properties that changed from optional to required.
    pub interface_optional_to_required: Vec<TsInterfacePropertyChange>,
    /// Interface properties whose nullability changed.
    pub interface_nullable_changes: Vec<TsInterfacePropertyChange>,
    /// Interface properties whose non-null type annotation changed.
    pub interface_type_changes: Vec<TsInterfacePropertyChange>,
    /// Public type declarations whose alias, heritage, enum, or enum-like value shape changed.
    pub type_declaration_changes: Vec<TsTypeDeclarationChange>,
    /// Operation/factory return type annotations changed or were removed.
    pub operation_return_type_changes: Vec<TsOperationReturnTypeChange>,
    /// Operation/factory method signatures changed or were removed.
    pub operation_signature_changes: Vec<TsOperationSignatureChange>,
    /// Package entry points changed or removed.
    pub package_entry_point_changes: Vec<String>,
    /// Documentation files present in old but missing in new.
    pub missing_docs: Vec<String>,
}

/// Extracted Python package surface.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct PythonSurface {
    /// Importable module paths, relative to the compared SDK root.
    pub modules: Vec<String>,
    /// Names re-exported by package `__init__.py` modules.
    pub public_exports: Vec<String>,
    /// Public model classes keyed by class name.
    pub models: BTreeMap<String, PythonModelSurface>,
    /// Exception classes available in the package.
    pub exception_classes: Vec<String>,
    /// Public type and model-field aliases.
    pub aliases: BTreeMap<String, String>,
    /// Packaging entry points from `pyproject.toml` or `setup.cfg`.
    pub package_entry_points: BTreeMap<String, String>,
}

/// One Python model class surface.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct PythonModelSurface {
    /// Annotated public model fields.
    pub fields: BTreeMap<String, PythonFieldSurface>,
    /// Explicit `__init__` signature, when the class declares one.
    pub constructor: Option<String>,
    /// Whether the model exposes `from_dict`.
    pub has_from_dict: bool,
    /// Whether the model exposes `to_dict`.
    pub has_to_dict: bool,
}

/// One Python model field surface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PythonFieldSurface {
    /// Whether callers must supply the field when constructing the model.
    pub required: bool,
    /// Whether the field annotation accepts `None`.
    pub nullable: bool,
    /// Canonicalized type annotation.
    pub ty: String,
    /// External/wire alias, when declared.
    pub alias: Option<String>,
}

/// A missing Python model field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PythonMissingModelField {
    /// Model class name.
    pub model: String,
    /// Field name.
    pub field: String,
}

/// A changed Python model field.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PythonModelFieldChange {
    /// Model class name.
    pub model: String,
    /// Field name.
    pub field: String,
    /// Baseline field shape.
    pub old: PythonFieldSurface,
    /// Candidate field shape.
    pub new: PythonFieldSurface,
}

/// A changed or removed explicit Python model constructor.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PythonConstructorChange {
    /// Model class name.
    pub model: String,
    /// Baseline normalized constructor signature.
    pub old: String,
    /// Candidate normalized constructor signature, or `<missing>`.
    pub new: String,
}

/// Python package surface diff report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct PythonSurfaceDiff {
    /// Baseline modules missing from the candidate package.
    pub missing_modules: Vec<String>,
    /// Baseline package exports missing from the candidate package.
    pub missing_public_exports: Vec<String>,
    /// Baseline model classes missing from the candidate package.
    pub missing_models: Vec<String>,
    /// Baseline model fields missing from candidate model classes.
    pub missing_model_fields: Vec<PythonMissingModelField>,
    /// Existing model fields whose requiredness, nullability, type, or alias changed.
    pub model_field_changes: Vec<PythonModelFieldChange>,
    /// Explicit model constructors that changed or disappeared.
    pub constructor_changes: Vec<PythonConstructorChange>,
    /// Models that lost a `from_dict` helper.
    pub missing_from_dict: Vec<String>,
    /// Models that lost a `to_dict` helper.
    pub missing_to_dict: Vec<String>,
    /// Baseline exception classes missing from the candidate package.
    pub missing_exception_classes: Vec<String>,
    /// Type or model-field aliases that changed or disappeared.
    pub alias_changes: Vec<String>,
    /// Package entry points that changed or disappeared.
    pub package_entry_point_changes: Vec<String>,
}

impl PythonSurfaceDiff {
    /// Whether this report contains a backwards-incompatible package change.
    #[must_use]
    pub fn is_breaking(&self) -> bool {
        !self.missing_modules.is_empty()
            || !self.missing_public_exports.is_empty()
            || !self.missing_models.is_empty()
            || !self.missing_model_fields.is_empty()
            || !self.model_field_changes.is_empty()
            || !self.constructor_changes.is_empty()
            || !self.missing_from_dict.is_empty()
            || !self.missing_to_dict.is_empty()
            || !self.missing_exception_classes.is_empty()
            || !self.alias_changes.is_empty()
            || !self.package_entry_point_changes.is_empty()
    }
}

impl TypeScriptSurfaceDiff {
    /// Whether this report contains any breaking change.
    #[must_use]
    pub fn is_breaking(&self) -> bool {
        !self.missing_root_exports.is_empty()
            || !self.missing_model_exports.is_empty()
            || !self.export_kind_mismatches.is_empty()
            || !self.missing_api_classes.is_empty()
            || !self.missing_api_factories.is_empty()
            || !self.missing_operation_methods.is_empty()
            || !self.missing_request_aliases.is_empty()
            || !self.missing_interface_properties.is_empty()
            || !self.interface_property_changes.is_empty()
            || !self.interface_required_to_optional.is_empty()
            || !self.interface_optional_to_required.is_empty()
            || !self.interface_nullable_changes.is_empty()
            || !self.interface_type_changes.is_empty()
            || !self.type_declaration_changes.is_empty()
            || !self.operation_return_type_changes.is_empty()
            || !self.operation_signature_changes.is_empty()
            || !self.package_entry_point_changes.is_empty()
            || !self.missing_docs.is_empty()
    }

    /// Whether this report contains code or package-surface breaking changes.
    #[must_use]
    pub fn has_code_breaks(&self) -> bool {
        !self.missing_root_exports.is_empty()
            || !self.missing_model_exports.is_empty()
            || !self.export_kind_mismatches.is_empty()
            || !self.missing_api_classes.is_empty()
            || !self.missing_api_factories.is_empty()
            || !self.missing_operation_methods.is_empty()
            || !self.missing_request_aliases.is_empty()
            || !self.missing_interface_properties.is_empty()
            || !self.interface_property_changes.is_empty()
            || !self.interface_required_to_optional.is_empty()
            || !self.interface_optional_to_required.is_empty()
            || !self.interface_nullable_changes.is_empty()
            || !self.interface_type_changes.is_empty()
            || !self.type_declaration_changes.is_empty()
            || !self.operation_return_type_changes.is_empty()
            || !self.operation_signature_changes.is_empty()
            || !self.package_entry_point_changes.is_empty()
    }

    /// Whether this report contains documentation-layout breaking changes.
    #[must_use]
    pub fn has_doc_breaks(&self) -> bool {
        !self.missing_docs.is_empty()
    }
}

/// Compatibility contract for SDK public-surface checks.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompatibilityContract {
    /// Cross-language compatibility allowances.
    pub allow: CompatibilityAllow,
    /// Go SDK compatibility requirements and approved drift.
    pub go: GoCompatibilityContract,
    /// TypeScript SDK compatibility requirements and approved drift.
    pub typescript: TypeScriptCompatibilityContract,
}

/// Cross-language compatibility allowances.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompatibilityAllow {
    /// Approve a one-time documentation layout migration for all missing docs.
    pub docs_layout_migration: bool,
    /// Missing documentation files that are explicitly approved for every SDK language.
    pub missing_docs: Vec<String>,
}

/// Go SDK compatibility requirements and approved drift.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GoCompatibilityContract {
    /// Exported types that must exist in the candidate SDK.
    pub require_exported_types: Vec<String>,
    /// Exported functions that must exist in the candidate SDK.
    pub require_exported_functions: Vec<String>,
    /// Exported methods that must exist in the candidate SDK.
    pub require_exported_methods: Vec<String>,
    /// Missing exported types that are explicitly approved.
    pub allow_missing_exported_types: Vec<String>,
    /// Missing exported functions that are explicitly approved.
    pub allow_missing_exported_functions: Vec<String>,
    /// Missing exported methods that are explicitly approved.
    pub allow_missing_exported_methods: Vec<String>,
    /// Exported type declaration changes that are explicitly approved, keyed by type name.
    pub allow_exported_type_changes: Vec<String>,
    /// Exported function signature changes that are explicitly approved, keyed by function name.
    pub allow_exported_function_signature_changes: Vec<String>,
    /// Exported method signature changes that are explicitly approved, keyed by `Receiver.Method`.
    pub allow_exported_method_signature_changes: Vec<String>,
    /// Missing documentation files that are explicitly approved.
    pub allow_missing_docs: Vec<String>,
    /// Package metadata changes that are explicitly approved.
    pub allow_package_metadata_changes: Vec<String>,
}

/// TypeScript SDK compatibility requirements and approved drift.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TypeScriptCompatibilityContract {
    /// Root exports that must exist in the candidate SDK.
    pub require_root_exports: Vec<String>,
    /// Model exports that must exist in the candidate SDK.
    pub require_model_exports: Vec<String>,
    /// API classes that must exist in the candidate SDK.
    pub require_api_classes: Vec<String>,
    /// API factories that must exist in the candidate SDK.
    pub require_api_factories: Vec<String>,
    /// Operation methods that must exist in the candidate SDK.
    pub require_operation_methods: Vec<String>,
    /// Request aliases that must exist in the candidate SDK.
    pub require_request_aliases: Vec<String>,
    /// Missing root exports that are explicitly approved.
    pub allow_missing_root_exports: Vec<String>,
    /// Missing model exports that are explicitly approved.
    pub allow_missing_model_exports: Vec<String>,
    /// Missing API classes that are explicitly approved.
    pub allow_missing_api_classes: Vec<String>,
    /// Missing API factories that are explicitly approved.
    pub allow_missing_api_factories: Vec<String>,
    /// Missing operation methods that are explicitly approved.
    pub allow_missing_operation_methods: Vec<String>,
    /// Missing request aliases that are explicitly approved.
    pub allow_missing_request_aliases: Vec<String>,
    /// Missing interface properties approved as `Interface.property`.
    pub allow_missing_interface_properties: Vec<String>,
    /// Interface property changes approved as `Interface.property`.
    pub allow_interface_property_changes: Vec<String>,
    /// Type declaration changes approved by exported declaration name.
    pub allow_type_declaration_changes: Vec<String>,
    /// Operation return type changes approved by operation key.
    pub allow_operation_return_type_changes: Vec<String>,
    /// Operation signature changes approved by operation key.
    pub allow_operation_signature_changes: Vec<String>,
    /// Export kind mismatches approved by symbol name.
    pub allow_export_kind_mismatches: Vec<String>,
    /// Package entry point changes that are explicitly approved.
    pub allow_package_entry_point_changes: Vec<String>,
    /// Missing documentation files that are explicitly approved.
    pub allow_missing_docs: Vec<String>,
}

/// Extracted Go public surface.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct GoSurface {
    /// Exported type names.
    pub exported_types: Vec<String>,
    /// Canonical exported type declarations keyed by type name.
    pub exported_type_declarations: BTreeMap<String, String>,
    /// Exported function signatures keyed by function name.
    pub exported_functions: BTreeMap<String, String>,
    /// Exported method signatures keyed by `Receiver.Method`.
    pub exported_methods: BTreeMap<String, String>,
    /// Documentation files present in the SDK package.
    pub docs: Vec<String>,
    /// `go.mod` metadata and other package-level compatibility fields.
    pub package_metadata: BTreeMap<String, String>,
}

/// Go surface diff report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct GoSurfaceDiff {
    /// Exported types present in old but missing in new.
    pub missing_exported_types: Vec<String>,
    /// Exported functions present in old but missing in new.
    pub missing_exported_functions: Vec<String>,
    /// Exported methods present in old but missing in new.
    pub missing_exported_methods: Vec<String>,
    /// Exported type declarations whose public shape changed.
    pub exported_type_changes: Vec<GoTypeDeclarationChange>,
    /// Exported function signatures changed.
    pub exported_function_signature_changes: Vec<GoSignatureChange>,
    /// Exported method signatures changed.
    pub exported_method_signature_changes: Vec<GoSignatureChange>,
    /// Documentation files present in old but missing in new.
    pub missing_docs: Vec<String>,
    /// Package metadata changed or was removed.
    pub package_metadata_changes: Vec<String>,
}

impl GoSurfaceDiff {
    /// Whether this report contains any breaking change.
    #[must_use]
    pub fn is_breaking(&self) -> bool {
        !self.missing_exported_types.is_empty()
            || !self.missing_exported_functions.is_empty()
            || !self.missing_exported_methods.is_empty()
            || !self.exported_type_changes.is_empty()
            || !self.exported_function_signature_changes.is_empty()
            || !self.exported_method_signature_changes.is_empty()
            || !self.missing_docs.is_empty()
            || !self.package_metadata_changes.is_empty()
    }

    /// Whether this report contains code or package-surface breaking changes.
    #[must_use]
    pub fn has_code_breaks(&self) -> bool {
        !self.missing_exported_types.is_empty()
            || !self.missing_exported_functions.is_empty()
            || !self.missing_exported_methods.is_empty()
            || !self.exported_type_changes.is_empty()
            || !self.exported_function_signature_changes.is_empty()
            || !self.exported_method_signature_changes.is_empty()
            || !self.package_metadata_changes.is_empty()
    }

    /// Whether this report contains documentation-layout breaking changes.
    #[must_use]
    pub fn has_doc_breaks(&self) -> bool {
        !self.missing_docs.is_empty()
    }
}

/// Go compatibility contract evaluation report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct GoContractEvaluation {
    /// Whether unapproved drift or missing required symbols remain.
    pub breaking: bool,
    /// Required symbols missing from the candidate SDK.
    pub missing_required: Vec<String>,
    /// Diff entries that were not approved by the contract.
    pub unapproved_diff: GoSurfaceDiff,
    /// Allowance entries that did not match any current diff item.
    pub stale_allowances: Vec<String>,
}

/// TypeScript compatibility contract evaluation report.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct TypeScriptContractEvaluation {
    /// Whether unapproved drift or missing required symbols remain.
    pub breaking: bool,
    /// Required symbols missing from the candidate SDK.
    pub missing_required: Vec<String>,
    /// Diff entries that were not approved by the contract.
    pub unapproved_diff: TypeScriptSurfaceDiff,
    /// Allowance entries that did not match any current diff item.
    pub stale_allowances: Vec<String>,
}

/// Changed Go function/method signature.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GoSignatureChange {
    /// Function or `Receiver.Method` symbol.
    pub symbol: String,
    /// Old normalized signature.
    pub old: String,
    /// New normalized signature.
    pub new: String,
}

/// Changed Go exported type declaration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GoTypeDeclarationChange {
    /// Exported type name.
    pub symbol: String,
    /// Old canonical declaration.
    pub old: String,
    /// New canonical declaration.
    pub new: String,
}

/// A type/value namespace mismatch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TsExportKindMismatch {
    /// Exported symbol.
    pub symbol: String,
    /// Old namespace kind.
    pub old: TsExportKind,
    /// New namespace kind.
    pub new: TsExportKind,
}

/// A TypeScript interface property declaration shape.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TsInterfaceProperty {
    /// Whether the property is declared with `?:`.
    pub optional: bool,
    /// Whether the property type includes `null`.
    pub nullable: bool,
    /// Normalized type annotation text.
    pub ty: String,
}

/// An interface property present in the old surface but missing in the new surface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TsMissingInterfaceProperty {
    /// Interface name.
    pub interface: String,
    /// Property name.
    pub property: String,
}

/// A changed TypeScript interface property declaration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TsInterfacePropertyChange {
    /// Interface name.
    pub interface: String,
    /// Property name.
    pub property: String,
    /// Old property shape.
    pub old: TsInterfaceProperty,
    /// New property shape.
    pub new: TsInterfaceProperty,
}

/// A changed or removed public TypeScript type declaration.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TsTypeDeclarationChange {
    /// Exported declaration name.
    pub symbol: String,
    /// Old canonical declaration shape.
    pub old: Vec<String>,
    /// New canonical declaration shapes; empty when the declaration disappeared.
    pub new: Vec<String>,
}

/// A changed TypeScript operation or factory return type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TsOperationReturnTypeChange {
    /// Operation key, e.g. `DefaultApi.listBooks`.
    pub operation: String,
    /// Old return type annotation.
    pub old: String,
    /// New return type annotation.
    pub new: String,
}

/// A changed TypeScript operation or factory method signature.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TsOperationSignatureChange {
    /// Operation key, e.g. `DefaultApi.listBooks`.
    pub operation: String,
    /// Old normalized method signature.
    pub old: String,
    /// New normalized method signature.
    pub new: String,
}

/// Diff two TypeScript SDK directories.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if either directory cannot be read.
pub fn diff_typescript_dirs(
    old_dir: impl AsRef<Path>,
    new_dir: impl AsRef<Path>,
) -> Result<TypeScriptSurfaceDiff, CoreError> {
    let old = extract_typescript_surface(old_dir)?;
    let new = extract_typescript_surface(new_dir)?;
    Ok(diff_typescript_surfaces(&old, &new))
}

/// Diff two Go SDK directories.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if either directory cannot be read.
pub fn diff_go_dirs(
    old_dir: impl AsRef<Path>,
    new_dir: impl AsRef<Path>,
) -> Result<GoSurfaceDiff, CoreError> {
    let old = extract_go_surface(old_dir)?;
    let new = extract_go_surface(new_dir)?;
    Ok(diff_go_surfaces(&old, &new))
}

/// Diff two generated Python SDK directories.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if either directory or one of its package files cannot be read.
pub fn diff_python_dirs(
    old_dir: impl AsRef<Path>,
    new_dir: impl AsRef<Path>,
) -> Result<PythonSurfaceDiff, CoreError> {
    let old = extract_python_surface(old_dir)?;
    let new = extract_python_surface(new_dir)?;
    Ok(diff_python_surfaces(&old, &new))
}

/// Evaluate a Go SDK diff against a compatibility contract.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "contract evaluation mirrors each Go diff and allow-list section explicitly"
)]
pub fn evaluate_go_contract(
    contract: &GoCompatibilityContract,
    diff: &GoSurfaceDiff,
    new: &GoSurface,
) -> GoContractEvaluation {
    let mut missing_required = Vec::new();
    require_values(
        "go.require_exported_types",
        &contract.require_exported_types,
        &new.exported_types,
        &mut missing_required,
    );
    require_map_keys(
        "go.require_exported_functions",
        &contract.require_exported_functions,
        &new.exported_functions,
        &mut missing_required,
    );
    require_map_keys(
        "go.require_exported_methods",
        &contract.require_exported_methods,
        &new.exported_methods,
        &mut missing_required,
    );

    let mut stale_allowances = Vec::new();
    stale_string_allowances(
        "go.allow_missing_exported_types",
        &contract.allow_missing_exported_types,
        &diff.missing_exported_types,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "go.allow_missing_exported_functions",
        &contract.allow_missing_exported_functions,
        &diff.missing_exported_functions,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "go.allow_missing_exported_methods",
        &contract.allow_missing_exported_methods,
        &diff.missing_exported_methods,
        &mut stale_allowances,
    );
    stale_go_type_allowances(
        "go.allow_exported_type_changes",
        &contract.allow_exported_type_changes,
        &diff.exported_type_changes,
        &mut stale_allowances,
    );
    stale_signature_allowances(
        "go.allow_exported_function_signature_changes",
        &contract.allow_exported_function_signature_changes,
        &diff.exported_function_signature_changes,
        &mut stale_allowances,
    );
    stale_signature_allowances(
        "go.allow_exported_method_signature_changes",
        &contract.allow_exported_method_signature_changes,
        &diff.exported_method_signature_changes,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "go.allow_missing_docs",
        &contract.allow_missing_docs,
        &diff.missing_docs,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "go.allow_package_metadata_changes",
        &contract.allow_package_metadata_changes,
        &diff.package_metadata_changes,
        &mut stale_allowances,
    );

    let unapproved_diff = GoSurfaceDiff {
        missing_exported_types: filter_allowed_strings(
            &diff.missing_exported_types,
            &contract.allow_missing_exported_types,
        ),
        missing_exported_functions: filter_allowed_strings(
            &diff.missing_exported_functions,
            &contract.allow_missing_exported_functions,
        ),
        missing_exported_methods: filter_allowed_strings(
            &diff.missing_exported_methods,
            &contract.allow_missing_exported_methods,
        ),
        exported_type_changes: filter_allowed_go_type_changes(
            &diff.exported_type_changes,
            &contract.allow_exported_type_changes,
        ),
        exported_function_signature_changes: filter_allowed_signature_changes(
            &diff.exported_function_signature_changes,
            &contract.allow_exported_function_signature_changes,
        ),
        exported_method_signature_changes: filter_allowed_signature_changes(
            &diff.exported_method_signature_changes,
            &contract.allow_exported_method_signature_changes,
        ),
        missing_docs: filter_allowed_strings(&diff.missing_docs, &contract.allow_missing_docs),
        package_metadata_changes: filter_allowed_strings(
            &diff.package_metadata_changes,
            &contract.allow_package_metadata_changes,
        ),
    };
    let breaking = !missing_required.is_empty() || unapproved_diff.is_breaking();
    GoContractEvaluation {
        breaking,
        missing_required,
        unapproved_diff,
        stale_allowances,
    }
}

/// Evaluate a TypeScript SDK diff against a compatibility contract.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "contract evaluation mirrors each TypeScript diff and allow-list section explicitly"
)]
pub fn evaluate_typescript_contract(
    contract: &TypeScriptCompatibilityContract,
    diff: &TypeScriptSurfaceDiff,
    new: &TypeScriptSurface,
) -> TypeScriptContractEvaluation {
    let mut missing_required = Vec::new();
    require_map_keys(
        "typescript.require_root_exports",
        &contract.require_root_exports,
        &new.root_exports,
        &mut missing_required,
    );
    require_map_keys(
        "typescript.require_model_exports",
        &contract.require_model_exports,
        &new.model_exports,
        &mut missing_required,
    );
    require_values(
        "typescript.require_api_classes",
        &contract.require_api_classes,
        &new.api_classes,
        &mut missing_required,
    );
    require_values(
        "typescript.require_api_factories",
        &contract.require_api_factories,
        &new.api_factories,
        &mut missing_required,
    );
    require_values(
        "typescript.require_operation_methods",
        &contract.require_operation_methods,
        &new.operation_methods,
        &mut missing_required,
    );
    require_values(
        "typescript.require_request_aliases",
        &contract.require_request_aliases,
        &new.request_aliases,
        &mut missing_required,
    );

    let mut stale_allowances = Vec::new();
    stale_string_allowances(
        "typescript.allow_missing_root_exports",
        &contract.allow_missing_root_exports,
        &diff.missing_root_exports,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "typescript.allow_missing_model_exports",
        &contract.allow_missing_model_exports,
        &diff.missing_model_exports,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "typescript.allow_missing_api_classes",
        &contract.allow_missing_api_classes,
        &diff.missing_api_classes,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "typescript.allow_missing_api_factories",
        &contract.allow_missing_api_factories,
        &diff.missing_api_factories,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "typescript.allow_missing_operation_methods",
        &contract.allow_missing_operation_methods,
        &diff.missing_operation_methods,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "typescript.allow_missing_request_aliases",
        &contract.allow_missing_request_aliases,
        &diff.missing_request_aliases,
        &mut stale_allowances,
    );
    stale_keyed_allowances(
        "typescript.allow_missing_interface_properties",
        &contract.allow_missing_interface_properties,
        diff.missing_interface_properties
            .iter()
            .map(ts_missing_interface_property_key),
        &mut stale_allowances,
    );
    stale_keyed_allowances(
        "typescript.allow_interface_property_changes",
        &contract.allow_interface_property_changes,
        diff.interface_property_changes
            .iter()
            .map(ts_interface_property_change_key),
        &mut stale_allowances,
    );
    stale_type_declaration_allowances(
        "typescript.allow_type_declaration_changes",
        &contract.allow_type_declaration_changes,
        &diff.type_declaration_changes,
        &mut stale_allowances,
    );
    stale_operation_return_allowances(
        "typescript.allow_operation_return_type_changes",
        &contract.allow_operation_return_type_changes,
        &diff.operation_return_type_changes,
        &mut stale_allowances,
    );
    stale_operation_signature_allowances(
        "typescript.allow_operation_signature_changes",
        &contract.allow_operation_signature_changes,
        &diff.operation_signature_changes,
        &mut stale_allowances,
    );
    stale_export_kind_allowances(
        "typescript.allow_export_kind_mismatches",
        &contract.allow_export_kind_mismatches,
        &diff.export_kind_mismatches,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "typescript.allow_package_entry_point_changes",
        &contract.allow_package_entry_point_changes,
        &diff.package_entry_point_changes,
        &mut stale_allowances,
    );
    stale_string_allowances(
        "typescript.allow_missing_docs",
        &contract.allow_missing_docs,
        &diff.missing_docs,
        &mut stale_allowances,
    );

    let unapproved_diff = TypeScriptSurfaceDiff {
        missing_root_exports: filter_allowed_strings(
            &diff.missing_root_exports,
            &contract.allow_missing_root_exports,
        ),
        missing_model_exports: filter_allowed_strings(
            &diff.missing_model_exports,
            &contract.allow_missing_model_exports,
        ),
        export_kind_mismatches: filter_allowed_export_kind_mismatches(
            &diff.export_kind_mismatches,
            &contract.allow_export_kind_mismatches,
        ),
        missing_api_classes: filter_allowed_strings(
            &diff.missing_api_classes,
            &contract.allow_missing_api_classes,
        ),
        missing_api_factories: filter_allowed_strings(
            &diff.missing_api_factories,
            &contract.allow_missing_api_factories,
        ),
        missing_operation_methods: filter_allowed_strings(
            &diff.missing_operation_methods,
            &contract.allow_missing_operation_methods,
        ),
        missing_request_aliases: filter_allowed_strings(
            &diff.missing_request_aliases,
            &contract.allow_missing_request_aliases,
        ),
        missing_interface_properties: filter_allowed_missing_interface_properties(
            &diff.missing_interface_properties,
            &contract.allow_missing_interface_properties,
        ),
        interface_property_changes: filter_allowed_interface_property_changes(
            &diff.interface_property_changes,
            &contract.allow_interface_property_changes,
        ),
        interface_required_to_optional: filter_allowed_interface_property_changes(
            &diff.interface_required_to_optional,
            &contract.allow_interface_property_changes,
        ),
        interface_optional_to_required: filter_allowed_interface_property_changes(
            &diff.interface_optional_to_required,
            &contract.allow_interface_property_changes,
        ),
        interface_nullable_changes: filter_allowed_interface_property_changes(
            &diff.interface_nullable_changes,
            &contract.allow_interface_property_changes,
        ),
        interface_type_changes: filter_allowed_interface_property_changes(
            &diff.interface_type_changes,
            &contract.allow_interface_property_changes,
        ),
        type_declaration_changes: filter_allowed_type_declaration_changes(
            &diff.type_declaration_changes,
            &contract.allow_type_declaration_changes,
        ),
        operation_return_type_changes: filter_allowed_operation_return_changes(
            &diff.operation_return_type_changes,
            &contract.allow_operation_return_type_changes,
        ),
        operation_signature_changes: filter_allowed_operation_signature_changes(
            &diff.operation_signature_changes,
            &contract.allow_operation_signature_changes,
        ),
        package_entry_point_changes: filter_allowed_strings(
            &diff.package_entry_point_changes,
            &contract.allow_package_entry_point_changes,
        ),
        missing_docs: filter_allowed_strings(&diff.missing_docs, &contract.allow_missing_docs),
    };
    let breaking = !missing_required.is_empty() || unapproved_diff.is_breaking();
    TypeScriptContractEvaluation {
        breaking,
        missing_required,
        unapproved_diff,
        stale_allowances,
    }
}

/// Extract a Go SDK public surface from a directory.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if the directory cannot be read.
pub fn extract_go_surface(dir: impl AsRef<Path>) -> Result<GoSurface, CoreError> {
    let dir = dir.as_ref();
    let mut files = Vec::new();
    collect_go_files(dir, dir, &mut files)?;
    files.sort();

    let mut exported_types = BTreeSet::new();
    let mut exported_type_declarations = BTreeMap::new();
    let mut exported_functions = BTreeMap::new();
    let mut exported_methods = BTreeMap::new();
    for rel in &files {
        let text = read_to_string(dir.join(rel))?;
        let parsed = parse_go_file(&text);
        exported_types.extend(
            parsed
                .type_declarations
                .keys()
                .map(|name| qualify_go_symbol(rel, name)),
        );
        exported_type_declarations.extend(
            parsed
                .type_declarations
                .into_iter()
                .map(|(name, declaration)| (qualify_go_symbol(rel, &name), declaration)),
        );
        exported_functions.extend(
            parsed
                .functions
                .into_iter()
                .map(|(name, signature)| (qualify_go_symbol(rel, &name), signature)),
        );
        exported_methods.extend(
            parsed
                .methods
                .into_iter()
                .map(|(name, signature)| (qualify_go_symbol(rel, &name), signature)),
        );
    }

    Ok(GoSurface {
        exported_types: exported_types.into_iter().collect(),
        exported_type_declarations,
        exported_functions,
        exported_methods,
        docs: doc_files(dir)?,
        package_metadata: go_package_metadata(dir)?,
    })
}

/// Diff two already-extracted Go surfaces.
#[must_use]
pub fn diff_go_surfaces(old: &GoSurface, new: &GoSurface) -> GoSurfaceDiff {
    GoSurfaceDiff {
        missing_exported_types: missing_values(&old.exported_types, &new.exported_types),
        missing_exported_functions: missing_string_keys(
            &old.exported_functions,
            &new.exported_functions,
        ),
        missing_exported_methods: missing_string_keys(&old.exported_methods, &new.exported_methods),
        exported_type_changes: go_type_declaration_changes(
            &old.exported_type_declarations,
            &new.exported_type_declarations,
        ),
        exported_function_signature_changes: go_signature_changes(
            &old.exported_functions,
            &new.exported_functions,
        ),
        exported_method_signature_changes: go_signature_changes(
            &old.exported_methods,
            &new.exported_methods,
        ),
        missing_docs: missing_values(&old.docs, &new.docs),
        package_metadata_changes: package_changes(&old.package_metadata, &new.package_metadata),
    }
}

/// Extract a Python SDK public surface from a directory.
///
/// The extractor is deliberately static: it does not import untrusted generated packages or require
/// their optional runtime dependencies. It follows package `__init__.py` exports and inspects typed
/// model declarations, explicit constructors, conversion helpers, exceptions, and packaging entry
/// points.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if the directory or a Python package file cannot be read.
pub fn extract_python_surface(dir: impl AsRef<Path>) -> Result<PythonSurface, CoreError> {
    let dir = dir.as_ref();
    let mut files = Vec::new();
    collect_python_files(dir, dir, &mut files)?;
    files.sort();
    let import_layout = python_import_layout(dir)?;
    let files = files
        .into_iter()
        .filter_map(|path| {
            import_layout
                .module_name(&path)
                .map(|module| (path, module))
        })
        .collect::<Vec<_>>();

    let modules = files.iter().map(|(_, module)| module.clone()).collect();
    let mut public_exports = BTreeSet::new();
    let mut models = BTreeMap::new();
    let mut exception_classes = BTreeSet::new();
    let mut aliases = BTreeMap::new();

    for (rel, module) in &files {
        let text = read_to_string(dir.join(rel))?;
        if rel.file_name().and_then(|name| name.to_str()) == Some("__init__.py") {
            public_exports.extend(parse_python_package_exports(rel, module, &text)?);
        }
        let parsed = parse_python_file(&text, is_python_model_file(rel));
        exception_classes.extend(parsed.exceptions);
        for (name, target) in parsed.aliases {
            if aliases.insert(name.clone(), target).is_some() {
                return Err(CoreError::Workspace {
                    message: format!("Python SDK surface contains duplicate public alias '{name}'"),
                });
            }
        }
        for (name, model) in parsed.models {
            if models.insert(name.clone(), model).is_some() {
                return Err(CoreError::Workspace {
                    message: format!(
                        "Python SDK surface contains duplicate public model class '{name}'"
                    ),
                });
            }
        }
    }

    Ok(PythonSurface {
        modules,
        public_exports: public_exports.into_iter().collect(),
        models,
        exception_classes: exception_classes.into_iter().collect(),
        aliases,
        package_entry_points: python_package_entry_points(dir)?,
    })
}

/// Diff two already-extracted Python package surfaces.
#[must_use]
pub fn diff_python_surfaces(old: &PythonSurface, new: &PythonSurface) -> PythonSurfaceDiff {
    let mut missing_model_fields = Vec::new();
    let mut model_field_changes = Vec::new();
    let mut constructor_changes = Vec::new();
    let mut missing_from_dict = Vec::new();
    let mut missing_to_dict = Vec::new();

    for (model_name, old_model) in &old.models {
        let Some(new_model) = new.models.get(model_name) else {
            continue;
        };
        for (field_name, old_field) in &old_model.fields {
            match new_model.fields.get(field_name) {
                Some(new_field) if old_field != new_field => {
                    model_field_changes.push(PythonModelFieldChange {
                        model: model_name.clone(),
                        field: field_name.clone(),
                        old: old_field.clone(),
                        new: new_field.clone(),
                    });
                }
                None => missing_model_fields.push(PythonMissingModelField {
                    model: model_name.clone(),
                    field: field_name.clone(),
                }),
                Some(_) => {}
            }
        }
        if let Some(old_constructor) = &old_model.constructor {
            if new_model.constructor.as_ref() != Some(old_constructor) {
                constructor_changes.push(PythonConstructorChange {
                    model: model_name.clone(),
                    old: old_constructor.clone(),
                    new: new_model
                        .constructor
                        .clone()
                        .unwrap_or_else(|| "<missing>".to_string()),
                });
            }
        }
        if old_model.has_from_dict && !new_model.has_from_dict {
            missing_from_dict.push(model_name.clone());
        }
        if old_model.has_to_dict && !new_model.has_to_dict {
            missing_to_dict.push(model_name.clone());
        }
    }

    PythonSurfaceDiff {
        missing_modules: missing_values(&old.modules, &new.modules),
        missing_public_exports: missing_values(&old.public_exports, &new.public_exports),
        missing_models: old
            .models
            .keys()
            .filter(|name| !new.models.contains_key(*name))
            .cloned()
            .collect(),
        missing_model_fields,
        model_field_changes,
        constructor_changes,
        missing_from_dict,
        missing_to_dict,
        missing_exception_classes: missing_values(&old.exception_classes, &new.exception_classes),
        alias_changes: package_changes(&old.aliases, &new.aliases),
        package_entry_point_changes: package_changes(
            &old.package_entry_points,
            &new.package_entry_points,
        ),
    }
}

/// Extract a TypeScript SDK surface from a directory.
///
/// # Errors
///
/// Returns [`CoreError::Workspace`] if the directory cannot be read.
pub fn extract_typescript_surface(dir: impl AsRef<Path>) -> Result<TypeScriptSurface, CoreError> {
    let dir = dir.as_ref();
    let mut files = Vec::new();
    collect_ts_files(dir, dir, &mut files)?;
    files.sort();

    let mut all_exports = BTreeMap::new();
    let mut model_exports = BTreeMap::new();
    let mut public = TsPublicSurfaceParts::default();
    let mut parsed_files = BTreeMap::new();

    for rel in &files {
        let text = read_to_string(dir.join(rel))?;
        parsed_files.insert(rel.clone(), parse_ts_file(&text));
    }

    let public_bindings = public_ts_export_bindings(dir, &files)?;
    let identifier_renames = unambiguous_ts_public_identifier_renames(&public_bindings);
    for (rel, parsed) in parsed_files {
        let bindings: BTreeMap<String, BTreeSet<String>> = public_bindings
            .iter()
            .filter(|(origin, _)| origin.file == rel)
            .map(|(origin, names)| (origin.symbol.clone(), names.clone()))
            .collect();
        if bindings.is_empty() {
            continue;
        }
        public.merge(&parsed, &bindings, &identifier_renames);
    }

    let mut export_cache = BTreeMap::new();
    for rel in &files {
        let exports = exports_from_file(dir, rel, &mut export_cache, &mut Vec::new())?;
        merge_exports(&mut all_exports, &exports);
        if is_model_file(rel) {
            merge_exports(&mut model_exports, &exports);
        }
    }

    let root_exports = extract_root_exports(dir, &files, &all_exports)?;
    Ok(TypeScriptSurface {
        root_exports,
        model_exports,
        api_classes: public.api_classes.into_iter().collect(),
        api_factories: public.api_factories.into_iter().collect(),
        operation_methods: public.operation_methods.into_iter().collect(),
        request_aliases: public.request_aliases.into_iter().collect(),
        interface_properties: public.interface_properties,
        type_declarations: public.type_declarations,
        operation_return_types: public.operation_return_types,
        operation_signatures: public.operation_signatures,
        package_entry_points: package_entry_points(dir)?,
        docs: doc_files(dir)?,
    })
}

#[derive(Default)]
struct TsPublicSurfaceParts {
    api_classes: BTreeSet<String>,
    api_factories: BTreeSet<String>,
    operation_methods: BTreeSet<String>,
    request_aliases: BTreeSet<String>,
    interface_properties: BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
    type_declarations: BTreeMap<String, Vec<String>>,
    operation_return_types: BTreeMap<String, String>,
    operation_signatures: BTreeMap<String, String>,
}

impl TsPublicSurfaceParts {
    fn merge(
        &mut self,
        parsed: &ParsedTsFile,
        bindings: &BTreeMap<String, BTreeSet<String>>,
        identifier_renames: &BTreeMap<String, String>,
    ) {
        for (origin, public_names) in bindings {
            for public_name in public_names {
                let mut renames = identifier_renames.clone();
                renames.insert(origin.clone(), public_name.clone());
                self.merge_binding(parsed, origin, public_name, &renames);
            }
        }
    }

    fn merge_binding(
        &mut self,
        parsed: &ParsedTsFile,
        origin: &str,
        public_name: &str,
        renames: &BTreeMap<String, String>,
    ) {
        if let Some(properties) = parsed.interface_properties.get(origin) {
            let properties = properties
                .iter()
                .map(|(name, property)| {
                    let mut property = property.clone();
                    property.ty = rename_ts_identifiers(&property.ty, renames);
                    (name.clone(), property)
                })
                .collect();
            merge_interface_properties(
                &mut self.interface_properties,
                BTreeMap::from([(public_name.to_string(), properties)]),
            );
        }
        if let Some(shapes) = parsed.type_declarations.get(origin) {
            merge_ts_declaration_shapes(
                &mut self.type_declarations,
                BTreeMap::from([(
                    public_name.to_string(),
                    shapes
                        .iter()
                        .map(|shape| rename_ts_identifiers(shape, renames))
                        .collect(),
                )]),
            );
        }

        for (operation, return_type) in &parsed.operation_return_types {
            if let Some(remapped) = remap_ts_operation_owner(operation, origin, public_name) {
                self.operation_return_types
                    .insert(remapped, rename_ts_identifiers(return_type, renames));
            }
        }
        for (operation, signature) in &parsed.operation_signatures {
            if let Some(remapped) = remap_ts_operation_owner(operation, origin, public_name) {
                self.operation_signatures
                    .insert(remapped, rename_ts_identifiers(signature, renames));
            }
        }
        for operation in &parsed.operation_methods {
            if let Some(remapped) = remap_ts_operation_owner(operation, origin, public_name) {
                self.operation_methods.insert(remapped);
            }
        }
        if parsed.api_classes.iter().any(|symbol| symbol == origin) {
            self.api_classes.insert(public_name.to_string());
        }
        if parsed.api_factories.iter().any(|symbol| symbol == origin) {
            self.api_factories.insert(public_name.to_string());
        }
        if parsed.request_aliases.iter().any(|symbol| symbol == origin) {
            self.request_aliases.insert(public_name.to_string());
        }
    }
}

fn remap_ts_operation_owner(operation: &str, origin: &str, public_name: &str) -> Option<String> {
    let (owner, method) = operation.split_once('.')?;
    (owner == origin).then(|| format!("{public_name}.{method}"))
}

/// Diff two already-extracted TypeScript surfaces.
#[must_use]
pub fn diff_typescript_surfaces(
    old: &TypeScriptSurface,
    new: &TypeScriptSurface,
) -> TypeScriptSurfaceDiff {
    let interface_property_changes =
        interface_property_changes(&old.interface_properties, &new.interface_properties);
    TypeScriptSurfaceDiff {
        missing_root_exports: missing_keys(&old.root_exports, &new.root_exports),
        missing_model_exports: missing_keys(&old.model_exports, &new.model_exports),
        export_kind_mismatches: kind_mismatches(&old.root_exports, &new.root_exports),
        missing_api_classes: missing_values(&old.api_classes, &new.api_classes),
        missing_api_factories: missing_values(&old.api_factories, &new.api_factories),
        missing_operation_methods: missing_values(&old.operation_methods, &new.operation_methods),
        missing_request_aliases: missing_values(&old.request_aliases, &new.request_aliases),
        missing_interface_properties: missing_interface_properties(
            &old.interface_properties,
            &new.interface_properties,
        ),
        interface_required_to_optional: interface_required_to_optional(&interface_property_changes),
        interface_optional_to_required: interface_optional_to_required(&interface_property_changes),
        interface_nullable_changes: interface_nullable_changes(&interface_property_changes),
        interface_type_changes: interface_type_changes(&interface_property_changes),
        interface_property_changes,
        type_declaration_changes: ts_type_declaration_changes(
            &old.type_declarations,
            &new.type_declarations,
        ),
        operation_return_type_changes: operation_return_type_changes(
            &old.operation_return_types,
            &new.operation_return_types,
        ),
        operation_signature_changes: operation_signature_changes(
            &old.operation_signatures,
            &new.operation_signatures,
        ),
        package_entry_point_changes: package_changes(
            &old.package_entry_points,
            &new.package_entry_points,
        ),
        missing_docs: missing_values(&old.docs, &new.docs),
    }
}

/// Suggest high-confidence config snippets for a Go compatibility diff.
#[must_use]
pub fn suggest_go_compat(diff: &GoSurfaceDiff) -> Vec<String> {
    let mut suggestions = BTreeSet::new();
    for change in &diff.exported_method_signature_changes {
        if let Some(request) = change.symbol.strip_suffix(".Execute") {
            suggestions.insert(format!(
                "GoExecuteCompatibility::preserve_legacy().request(\"{request}\")"
            ));
            continue;
        }
        let Some((request, setter)) = change.symbol.split_once('.') else {
            continue;
        };
        if request.ends_with("Request") && go_signature_has_any_parameter(&change.old) {
            suggestions.insert(format!(
                "GoQuerySetterArgumentPolicy::typed().any_for(\"{request}\", \"{setter}\")"
            ));
        }
    }
    for missing in &diff.missing_exported_methods {
        let Some((request, setter)) = missing.split_once('.') else {
            continue;
        };
        if request.ends_with("Request") {
            suggestions.insert(format!(
                "GoRequestBuilderAliases::new().body(\"{request}\", \"{setter}\")"
            ));
        }
    }
    suggestions.into_iter().collect()
}

fn go_signature_has_any_parameter(signature: &str) -> bool {
    signature.contains("(any)")
        || signature.contains("(any,")
        || signature.contains(", any)")
        || signature.contains(", any,")
        || signature.contains(" any)")
        || signature.contains(" any,")
}

/// Suggest high-confidence config snippets for a TypeScript compatibility diff.
#[must_use]
pub fn suggest_typescript_compat(diff: &TypeScriptSurfaceDiff) -> Vec<String> {
    let mut suggestions = BTreeSet::new();
    if !diff.operation_return_type_changes.is_empty() {
        suggestions
            .insert("TsSdk::new().compatibility(TsCompatibility::OpenApiGenerator)".to_string());
        suggestions.insert(".response_policy(TsResponsePolicy::AxiosResponseWrapper)".to_string());
    }
    if !diff.interface_optional_to_required.is_empty()
        || !diff.interface_required_to_optional.is_empty()
    {
        suggestions.insert(
            ".model_property_policy(TsModelPropertyPolicy::OpenApiGeneratorLoose)".to_string(),
        );
    }
    if !diff.interface_nullable_changes.is_empty() {
        suggestions.insert(
            ".nullable_policy(TsNullablePolicy::OmitNullFromOptionalProperties)".to_string(),
        );
    }
    suggestions.into_iter().collect()
}

fn require_values(
    label: &str,
    required: &[String],
    available: &[String],
    missing: &mut Vec<String>,
) {
    let available: BTreeSet<&str> = available.iter().map(String::as_str).collect();
    for item in required {
        if !available.contains(item.as_str()) {
            missing.push(format!("{label}: {item}"));
        }
    }
}

fn require_map_keys<T>(
    label: &str,
    required: &[String],
    available: &BTreeMap<String, T>,
    missing: &mut Vec<String>,
) {
    for item in required {
        if !available.contains_key(item) {
            missing.push(format!("{label}: {item}"));
        }
    }
}

fn filter_allowed_strings(values: &[String], allowed: &[String]) -> Vec<String> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    values
        .iter()
        .filter(|value| !allowed.contains(value.as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_signature_changes(
    changes: &[GoSignatureChange],
    allowed: &[String],
) -> Vec<GoSignatureChange> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(change.symbol.as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_go_type_changes(
    changes: &[GoTypeDeclarationChange],
    allowed: &[String],
) -> Vec<GoTypeDeclarationChange> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(change.symbol.as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_export_kind_mismatches(
    changes: &[TsExportKindMismatch],
    allowed: &[String],
) -> Vec<TsExportKindMismatch> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(change.symbol.as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_missing_interface_properties(
    changes: &[TsMissingInterfaceProperty],
    allowed: &[String],
) -> Vec<TsMissingInterfaceProperty> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(ts_missing_interface_property_key(change).as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_interface_property_changes(
    changes: &[TsInterfacePropertyChange],
    allowed: &[String],
) -> Vec<TsInterfacePropertyChange> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(ts_interface_property_change_key(change).as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_type_declaration_changes(
    changes: &[TsTypeDeclarationChange],
    allowed: &[String],
) -> Vec<TsTypeDeclarationChange> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(change.symbol.as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_operation_return_changes(
    changes: &[TsOperationReturnTypeChange],
    allowed: &[String],
) -> Vec<TsOperationReturnTypeChange> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(change.operation.as_str()))
        .cloned()
        .collect()
}

fn filter_allowed_operation_signature_changes(
    changes: &[TsOperationSignatureChange],
    allowed: &[String],
) -> Vec<TsOperationSignatureChange> {
    let allowed: BTreeSet<&str> = allowed.iter().map(String::as_str).collect();
    changes
        .iter()
        .filter(|change| !allowed.contains(change.operation.as_str()))
        .cloned()
        .collect()
}

fn stale_string_allowances(
    label: &str,
    allowed: &[String],
    current: &[String],
    stale: &mut Vec<String>,
) {
    stale_keyed_allowances(label, allowed, current.iter().cloned(), stale);
}

fn stale_signature_allowances(
    label: &str,
    allowed: &[String],
    current: &[GoSignatureChange],
    stale: &mut Vec<String>,
) {
    stale_keyed_allowances(
        label,
        allowed,
        current.iter().map(|change| change.symbol.clone()),
        stale,
    );
}

fn stale_go_type_allowances(
    label: &str,
    allowed: &[String],
    current: &[GoTypeDeclarationChange],
    stale: &mut Vec<String>,
) {
    stale_keyed_allowances(
        label,
        allowed,
        current.iter().map(|change| change.symbol.clone()),
        stale,
    );
}

fn stale_export_kind_allowances(
    label: &str,
    allowed: &[String],
    current: &[TsExportKindMismatch],
    stale: &mut Vec<String>,
) {
    stale_keyed_allowances(
        label,
        allowed,
        current.iter().map(|change| change.symbol.clone()),
        stale,
    );
}

fn stale_type_declaration_allowances(
    label: &str,
    allowed: &[String],
    current: &[TsTypeDeclarationChange],
    stale: &mut Vec<String>,
) {
    stale_keyed_allowances(
        label,
        allowed,
        current.iter().map(|change| change.symbol.clone()),
        stale,
    );
}

fn stale_operation_return_allowances(
    label: &str,
    allowed: &[String],
    current: &[TsOperationReturnTypeChange],
    stale: &mut Vec<String>,
) {
    stale_keyed_allowances(
        label,
        allowed,
        current.iter().map(|change| change.operation.clone()),
        stale,
    );
}

fn stale_operation_signature_allowances(
    label: &str,
    allowed: &[String],
    current: &[TsOperationSignatureChange],
    stale: &mut Vec<String>,
) {
    stale_keyed_allowances(
        label,
        allowed,
        current.iter().map(|change| change.operation.clone()),
        stale,
    );
}

fn stale_keyed_allowances<I>(label: &str, allowed: &[String], current: I, stale: &mut Vec<String>)
where
    I: IntoIterator<Item = String>,
{
    let current: BTreeSet<String> = current.into_iter().collect();
    for item in allowed {
        if !current.contains(item) {
            stale.push(format!("{label}: {item}"));
        }
    }
}

fn ts_missing_interface_property_key(change: &TsMissingInterfaceProperty) -> String {
    format!("{}.{}", change.interface, change.property)
}

fn ts_interface_property_change_key(change: &TsInterfacePropertyChange) -> String {
    format!("{}.{}", change.interface, change.property)
}

#[derive(Default)]
struct ParsedTsFile {
    exports: BTreeMap<String, TsExportKind>,
    export_origins: BTreeMap<String, BTreeSet<String>>,
    declaration_kinds: BTreeMap<String, TsExportKind>,
    api_classes: Vec<String>,
    api_factories: Vec<String>,
    operation_methods: Vec<String>,
    request_aliases: Vec<String>,
    interface_properties: BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
    type_declarations: BTreeMap<String, Vec<String>>,
    operation_return_types: BTreeMap<String, String>,
    operation_signatures: BTreeMap<String, String>,
}

#[derive(Default)]
struct TsInterfaceState {
    name: String,
    depth: i32,
    opened: bool,
    property: String,
}

#[derive(Default)]
struct TsContainerState {
    name: String,
    depth: i32,
    opened: bool,
    pending_method: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TsDeclarationKind {
    Interface,
    TypeAlias,
    Class,
    Const,
    Enum,
    Other,
}

struct TsParsedDeclaration<'a> {
    name: Option<&'a str>,
    export_name: &'a str,
    export_kind: TsExportKind,
    declaration_kind: TsDeclarationKind,
    exported: bool,
}

struct TsParsedMethod {
    name: String,
    return_type: Option<String>,
    signature: String,
}

enum TsMethodParse {
    Incomplete,
    NotMethod,
    Method(TsParsedMethod),
}

#[derive(Clone, Copy)]
enum TsDeclarationShapeKind {
    Header,
    Statement,
    ConstStatement,
    Enum,
}

struct TsDeclarationShapeState {
    symbol: String,
    kind: TsDeclarationShapeKind,
    code: String,
    structure: String,
}

enum TsDeclarationShapeProgress {
    Incomplete,
    Ignored,
    Complete(String),
}

#[expect(
    clippy::too_many_lines,
    reason = "single-pass parser state is easier to audit together"
)]
fn parse_ts_file(text: &str) -> ParsedTsFile {
    let mut parsed = ParsedTsFile::default();
    let mut current_api_class: Option<TsContainerState> = None;
    let mut current_api_factory: Option<TsContainerState> = None;
    let mut current_interface: Option<TsInterfaceState> = None;
    let code = sanitize_typescript(text, false);
    let structure = sanitize_typescript(text, true);
    let mut top_level_depth = 0_i32;
    for (raw, structural_raw) in code.lines().zip(structure.lines()) {
        let line = raw.trim();
        let structural_line = structural_raw.trim();
        let mut starts_api_class = false;
        let mut starts_api_factory = false;
        let mut starts_interface = false;
        if let Some(declaration) = (top_level_depth == 0)
            .then(|| parse_ts_declaration(line))
            .flatten()
        {
            if let Some(name) = declaration.name {
                parsed
                    .declaration_kinds
                    .insert(name.to_string(), declaration.export_kind);
                if declaration.exported {
                    add_export(
                        &mut parsed.exports,
                        declaration.export_name,
                        declaration.export_kind,
                    );
                    parsed
                        .export_origins
                        .entry(declaration.export_name.to_string())
                        .or_default()
                        .insert(name.to_string());
                }
                if matches!(
                    declaration.declaration_kind,
                    TsDeclarationKind::Interface | TsDeclarationKind::TypeAlias
                ) && name.ends_with("Request")
                {
                    parsed.request_aliases.push(name.to_string());
                }
                if declaration.declaration_kind == TsDeclarationKind::Interface {
                    parsed
                        .interface_properties
                        .entry(name.to_string())
                        .or_default();
                    let depth = brace_delta(structural_line);
                    current_interface = Some(TsInterfaceState {
                        name: name.to_string(),
                        depth,
                        opened: depth > 0,
                        property: String::new(),
                    });
                    starts_interface = true;
                } else if declaration.declaration_kind == TsDeclarationKind::Class
                    && name.ends_with("Api")
                {
                    let depth = brace_delta(structural_line);
                    current_api_class = Some(TsContainerState {
                        name: name.to_string(),
                        depth,
                        opened: depth > 0,
                        pending_method: String::new(),
                    });
                    starts_api_class = true;
                    parsed.api_classes.push(name.to_string());
                } else if declaration.declaration_kind == TsDeclarationKind::Const
                    && name.ends_with("ApiFactory")
                {
                    let depth = brace_delta(structural_line);
                    parsed.api_factories.push(name.to_string());
                    current_api_factory = Some(TsContainerState {
                        name: name.to_string(),
                        depth,
                        opened: depth > 0,
                        pending_method: String::new(),
                    });
                    starts_api_factory = true;
                }
            } else if declaration.exported {
                add_export(
                    &mut parsed.exports,
                    declaration.export_name,
                    declaration.export_kind,
                );
            }
        }

        let mut close_api_class = false;
        if let Some(class) = &mut current_api_class {
            if !starts_api_class {
                if class.opened && (class.depth == 1 || !class.pending_method.is_empty()) {
                    if let Some(method) = collect_ts_method(class, line) {
                        let key = format!("{}.{}", class.name, method.name);
                        parsed.operation_methods.push(key.clone());
                        if let Some(return_ty) = method.return_type {
                            parsed.operation_return_types.insert(key.clone(), return_ty);
                        }
                        parsed.operation_signatures.insert(key, method.signature);
                    }
                }
                class.depth += brace_delta(structural_line);
                class.opened |= class.depth > 0;
            }
            if class.opened && class.depth <= 0 {
                close_api_class = true;
            }
        }
        if close_api_class {
            current_api_class = None;
        }

        let mut close_api_factory = false;
        if let Some(factory) = &mut current_api_factory {
            if !starts_api_factory {
                if factory.opened {
                    if let Some(method) = collect_ts_method(factory, line) {
                        if let Some(return_ty) = method.return_type {
                            let key = format!("{}.{}", factory.name, method.name);
                            parsed.operation_return_types.insert(key.clone(), return_ty);
                            parsed.operation_signatures.insert(key, method.signature);
                        }
                    }
                }
                factory.depth += brace_delta(structural_line);
                factory.opened |= factory.depth > 0;
            }
            if factory.opened && factory.depth <= 0 {
                close_api_factory = true;
            }
        }
        if close_api_factory {
            current_api_factory = None;
        }

        let mut close_interface = false;
        if let Some(interface) = &mut current_interface {
            if !starts_interface {
                if interface.opened && (interface.depth == 1 || !interface.property.is_empty()) {
                    collect_interface_property(line, interface, &mut parsed.interface_properties);
                }
                interface.depth += brace_delta(structural_line);
                interface.opened |= interface.depth > 0;
            }
            if interface.opened && interface.depth <= 0 {
                close_interface = true;
            }
        }
        if close_interface {
            current_interface = None;
        }
        top_level_depth += brace_delta(structural_line);
    }
    parsed.type_declarations = extract_ts_declaration_shapes(text);
    parsed
}

fn collect_ts_method(state: &mut TsContainerState, line: &str) -> Option<TsParsedMethod> {
    if state.pending_method.is_empty() {
        if !could_start_ts_method(line) {
            return None;
        }
        state.pending_method.push_str(line);
    } else {
        state.pending_method.push(' ');
        state.pending_method.push_str(line);
    }
    match parse_ts_method_declaration(&state.pending_method) {
        TsMethodParse::Incomplete => None,
        TsMethodParse::NotMethod => {
            state.pending_method.clear();
            None
        }
        TsMethodParse::Method(method) => {
            state.pending_method.clear();
            Some(method)
        }
    }
}

fn extract_ts_declaration_shapes(text: &str) -> BTreeMap<String, Vec<String>> {
    let code = sanitize_typescript(text, false);
    let structure = sanitize_typescript(text, true);
    let mut declarations = BTreeMap::new();
    let mut pending: Option<TsDeclarationShapeState> = None;
    let mut top_level_depth = 0_i32;
    for (raw, structural_raw) in code.lines().zip(structure.lines()) {
        if let Some(state) = &mut pending {
            append_ts_declaration_shape_line(state, raw, structural_raw);
            match finish_ts_declaration_shape(state) {
                TsDeclarationShapeProgress::Complete(shape) => {
                    add_ts_declaration_shape(&mut declarations, &state.symbol, shape);
                    pending = None;
                }
                TsDeclarationShapeProgress::Ignored => pending = None,
                TsDeclarationShapeProgress::Incomplete => {}
            }
            top_level_depth += brace_delta(structural_raw);
            continue;
        }

        if top_level_depth != 0 {
            top_level_depth += brace_delta(structural_raw);
            continue;
        }

        let line = raw.trim();
        let Some(declaration) = parse_ts_declaration(line) else {
            top_level_depth += brace_delta(structural_raw);
            continue;
        };
        let Some(name) = declaration.name else {
            top_level_depth += brace_delta(structural_raw);
            continue;
        };
        let kind = match declaration.declaration_kind {
            TsDeclarationKind::Interface | TsDeclarationKind::Class => {
                TsDeclarationShapeKind::Header
            }
            TsDeclarationKind::TypeAlias => TsDeclarationShapeKind::Statement,
            TsDeclarationKind::Const if !name.ends_with("ApiFactory") => {
                TsDeclarationShapeKind::ConstStatement
            }
            TsDeclarationKind::Enum => TsDeclarationShapeKind::Enum,
            TsDeclarationKind::Const | TsDeclarationKind::Other => {
                top_level_depth += brace_delta(structural_raw);
                continue;
            }
        };
        let mut state = TsDeclarationShapeState {
            symbol: name.to_string(),
            kind,
            code: String::new(),
            structure: String::new(),
        };
        append_ts_declaration_shape_line(&mut state, raw, structural_raw);
        match finish_ts_declaration_shape(&state) {
            TsDeclarationShapeProgress::Complete(shape) => {
                add_ts_declaration_shape(&mut declarations, &state.symbol, shape);
            }
            TsDeclarationShapeProgress::Ignored => {}
            TsDeclarationShapeProgress::Incomplete => pending = Some(state),
        }
        top_level_depth += brace_delta(structural_raw);
    }
    declarations
}

fn append_ts_declaration_shape_line(
    state: &mut TsDeclarationShapeState,
    code: &str,
    structure: &str,
) {
    if !state.code.is_empty() {
        state.code.push('\n');
        state.structure.push('\n');
    }
    state.code.push_str(code);
    state.structure.push_str(structure);
}

fn finish_ts_declaration_shape(state: &TsDeclarationShapeState) -> TsDeclarationShapeProgress {
    let end = match state.kind {
        TsDeclarationShapeKind::Header => ts_top_level_body_open(&state.structure),
        TsDeclarationShapeKind::Statement | TsDeclarationShapeKind::ConstStatement => {
            ts_top_level_semicolon(&state.structure)
        }
        TsDeclarationShapeKind::Enum => ts_top_level_body_open(&state.structure)
            .and_then(|open| ts_matching_brace_end(&state.structure, open))
            .map(|end| end + 1),
    };
    let Some(end) = end else {
        return TsDeclarationShapeProgress::Incomplete;
    };
    let prefix = corresponding_ts_code_prefix(&state.code, &state.structure, end);
    let shape = canonical_ts_declaration(&prefix);
    if matches!(state.kind, TsDeclarationShapeKind::ConstStatement) && !shape.contains("as const") {
        return TsDeclarationShapeProgress::Ignored;
    }
    if shape.is_empty() {
        TsDeclarationShapeProgress::Ignored
    } else {
        TsDeclarationShapeProgress::Complete(shape)
    }
}

fn ts_top_level_body_open(value: &str) -> Option<usize> {
    let mut angle_depth = 0_u32;
    let mut square_depth = 0_u32;
    let mut paren_depth = 0_u32;
    for (idx, ch) in value.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '[' => square_depth += 1,
            ']' if square_depth > 0 => square_depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            '{' if angle_depth == 0 && square_depth == 0 && paren_depth == 0 => {
                return Some(idx);
            }
            _ => {}
        }
    }
    None
}

fn ts_top_level_semicolon(value: &str) -> Option<usize> {
    let mut angle_depth = 0_u32;
    let mut square_depth = 0_u32;
    let mut paren_depth = 0_u32;
    let mut brace_depth = 0_u32;
    for (idx, ch) in value.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '[' => square_depth += 1,
            ']' if square_depth > 0 => square_depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            '{' => brace_depth += 1,
            '}' if brace_depth > 0 => brace_depth -= 1,
            ';' if angle_depth == 0
                && square_depth == 0
                && paren_depth == 0
                && brace_depth == 0 =>
            {
                return Some(idx);
            }
            _ => {}
        }
    }
    None
}

fn ts_matching_brace_end(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0_u32;
    for (idx, ch) in value.char_indices().filter(|(idx, _)| *idx >= open) {
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

fn corresponding_ts_code_prefix(code: &str, structure: &str, structural_end: usize) -> String {
    let char_count = structure[..structural_end].chars().count();
    code.chars().take(char_count).collect()
}

fn canonical_ts_declaration(declaration: &str) -> String {
    let mut rest = strip_ts_keyword(declaration, "export").unwrap_or(declaration.trim_start());
    while let Some(next) =
        strip_ts_keyword(rest, "default").or_else(|| strip_ts_keyword(rest, "declare"))
    {
        rest = next;
    }
    normalize_ts_tokens(rest.trim().trim_end_matches(';').trim())
}

fn collect_interface_property(
    line: &str,
    state: &mut TsInterfaceState,
    properties: &mut BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
) {
    if state.property.is_empty()
        && (line.is_empty()
            || line.starts_with("//")
            || line.starts_with("/*")
            || line.starts_with('*')
            || line.starts_with('[')
            || line.starts_with('{')
            || line.starts_with('}'))
    {
        return;
    }
    if !state.property.is_empty() {
        state.property.push(' ');
    }
    state.property.push_str(line);
    if !interface_property_decl_complete(&state.property) {
        return;
    }
    if let Some((property, shape)) = parse_interface_property(&state.property) {
        properties
            .entry(state.name.clone())
            .or_default()
            .insert(property, shape);
    }
    state.property.clear();
}

fn interface_property_decl_complete(decl: &str) -> bool {
    let mut quote = None;
    let mut escape = false;
    let mut angle_depth = 0_u32;
    let mut square_depth = 0_u32;
    let mut brace_depth = 0_u32;
    let mut paren_depth = 0_u32;
    for ch in decl.chars() {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '[' => square_depth += 1,
            ']' if square_depth > 0 => square_depth -= 1,
            '{' => brace_depth += 1,
            '}' if brace_depth > 0 => brace_depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            ';' | ','
                if angle_depth == 0
                    && square_depth == 0
                    && brace_depth == 0
                    && paren_depth == 0 =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

#[derive(Clone, Copy)]
enum TsSanitizeState {
    Code,
    LineComment,
    BlockComment,
    SingleQuote,
    DoubleQuote,
    Template,
    Regex,
}

impl TsSanitizeState {
    fn quote_delimiter(self) -> Option<char> {
        match self {
            Self::SingleQuote => Some('\''),
            Self::DoubleQuote => Some('"'),
            Self::Template => Some('`'),
            Self::Code | Self::LineComment | Self::BlockComment | Self::Regex => None,
        }
    }
}

fn sanitize_typescript(text: &str, erase_strings: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut state = TsSanitizeState::Code;
    let mut regex_escape = false;
    let mut regex_character_class = false;
    while let Some(ch) = chars.next() {
        match state {
            TsSanitizeState::Code => match (ch, chars.peek().copied()) {
                ('/', Some('/')) => {
                    chars.next();
                    out.push_str("  ");
                    state = TsSanitizeState::LineComment;
                }
                ('/', Some('*')) => {
                    chars.next();
                    out.push_str("  ");
                    state = TsSanitizeState::BlockComment;
                }
                ('/', _) if ts_slash_starts_regex(&out) => {
                    out.push(if erase_strings { ' ' } else { ch });
                    regex_escape = false;
                    regex_character_class = false;
                    state = TsSanitizeState::Regex;
                }
                ('\'', _) => {
                    out.push(if erase_strings { ' ' } else { ch });
                    state = TsSanitizeState::SingleQuote;
                }
                ('"', _) => {
                    out.push(if erase_strings { ' ' } else { ch });
                    state = TsSanitizeState::DoubleQuote;
                }
                ('`', _) => {
                    out.push(if erase_strings { ' ' } else { ch });
                    state = TsSanitizeState::Template;
                }
                _ => out.push(ch),
            },
            TsSanitizeState::LineComment => {
                if ch == '\n' {
                    out.push(ch);
                    state = TsSanitizeState::Code;
                } else {
                    out.push(' ');
                }
            }
            TsSanitizeState::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    out.push_str("  ");
                    state = TsSanitizeState::Code;
                } else {
                    out.push(if ch == '\n' { '\n' } else { ' ' });
                }
            }
            TsSanitizeState::SingleQuote
            | TsSanitizeState::DoubleQuote
            | TsSanitizeState::Template => {
                let Some(closing) = state.quote_delimiter() else {
                    continue;
                };
                out.push(if erase_strings && ch != '\n' { ' ' } else { ch });
                if ch == '\\' {
                    if let Some(escaped) = chars.next() {
                        out.push(if erase_strings && escaped != '\n' {
                            ' '
                        } else {
                            escaped
                        });
                    }
                } else if ch == closing || (ch == '\n' && closing != '`') {
                    state = TsSanitizeState::Code;
                }
            }
            TsSanitizeState::Regex => {
                out.push(if erase_strings && ch != '\n' { ' ' } else { ch });
                if regex_escape {
                    regex_escape = false;
                } else if ch == '\\' {
                    regex_escape = true;
                } else if ch == '[' {
                    regex_character_class = true;
                } else if ch == ']' {
                    regex_character_class = false;
                } else if (ch == '/' && !regex_character_class) || ch == '\n' {
                    state = TsSanitizeState::Code;
                }
            }
        }
    }
    out
}

fn ts_slash_starts_regex(output: &str) -> bool {
    let trimmed = output.trim_end();
    let Some(previous) = trimmed.chars().next_back() else {
        return true;
    };
    if matches!(
        previous,
        '=' | '('
            | ':'
            | ','
            | '!'
            | '['
            | '{'
            | ';'
            | '?'
            | '&'
            | '|'
            | '*'
            | '%'
            | '^'
            | '~'
            | '<'
            | '>'
    ) {
        return true;
    }
    let preceding_word: String = trimmed
        .chars()
        .rev()
        .take_while(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '$')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    matches!(
        preceding_word.as_str(),
        "return"
            | "throw"
            | "case"
            | "delete"
            | "void"
            | "typeof"
            | "yield"
            | "await"
            | "else"
            | "do"
    )
}

fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |delta, ch| match ch {
        '{' => delta + 1,
        '}' => delta - 1,
        _ => delta,
    })
}

fn parse_ts_declaration(line: &str) -> Option<TsParsedDeclaration<'_>> {
    let (mut rest, exported) =
        strip_ts_keyword(line, "export").map_or((line.trim_start(), false), |rest| (rest, true));
    let mut is_default = false;
    loop {
        if let Some(next) = strip_ts_keyword(rest, "default") {
            if !exported {
                return None;
            }
            is_default = true;
            rest = next;
        } else if let Some(next) = strip_ts_keyword(rest, "declare")
            .or_else(|| strip_ts_keyword(rest, "abstract"))
            .or_else(|| strip_ts_keyword(rest, "async"))
        {
            rest = next;
        } else {
            break;
        }
    }

    let (declaration_kind, export_kind, after_kind) =
        if let Some(after_const) = strip_ts_keyword(rest, "const") {
            if let Some(after_enum) = strip_ts_keyword(after_const, "enum") {
                (TsDeclarationKind::Enum, TsExportKind::Both, after_enum)
            } else {
                (TsDeclarationKind::Const, TsExportKind::Value, after_const)
            }
        } else if let Some(after) = strip_ts_keyword(rest, "interface") {
            (TsDeclarationKind::Interface, TsExportKind::Type, after)
        } else if let Some(after) = strip_ts_keyword(rest, "type") {
            (TsDeclarationKind::TypeAlias, TsExportKind::Type, after)
        } else if let Some(after) = strip_ts_keyword(rest, "class") {
            (TsDeclarationKind::Class, TsExportKind::Both, after)
        } else if let Some(after) = strip_ts_keyword(rest, "enum") {
            (TsDeclarationKind::Enum, TsExportKind::Both, after)
        } else if let Some(after) =
            strip_ts_keyword(rest, "namespace").or_else(|| strip_ts_keyword(rest, "module"))
        {
            (TsDeclarationKind::Other, TsExportKind::Both, after)
        } else if let Some(after) = strip_ts_keyword(rest, "function")
            .or_else(|| strip_ts_keyword(rest, "let"))
            .or_else(|| strip_ts_keyword(rest, "var"))
        {
            (TsDeclarationKind::Other, TsExportKind::Value, after)
        } else if is_default {
            return Some(TsParsedDeclaration {
                name: None,
                export_name: "default",
                export_kind: TsExportKind::Value,
                declaration_kind: TsDeclarationKind::Other,
                exported,
            });
        } else {
            return None;
        };

    let name = ident_prefix(after_kind);
    if name.is_none() && !is_default {
        return None;
    }
    Some(TsParsedDeclaration {
        name,
        export_name: if is_default { "default" } else { name? },
        export_kind,
        declaration_kind,
        exported,
    })
}

fn strip_ts_keyword<'a>(value: &'a str, keyword: &str) -> Option<&'a str> {
    let value = value.trim_start();
    let rest = value.strip_prefix(keyword)?;
    if rest
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
    {
        return None;
    }
    Some(rest.trim_start())
}

fn could_start_ts_method(line: &str) -> bool {
    let mut rest = line.trim_start();
    loop {
        let Some(modifier) = ident_prefix(rest) else {
            return false;
        };
        match modifier {
            "private" | "protected" | "get" | "set" => return false,
            "public" | "static" | "async" | "override" | "abstract" | "declare" => {
                rest = rest[modifier.len()..].trim_start();
            }
            _ => break,
        }
    }
    let Some(name) = ident_prefix(rest) else {
        return false;
    };
    if matches!(
        name,
        "constructor" | "if" | "for" | "while" | "switch" | "catch" | "function" | "return" | "new"
    ) {
        return false;
    }
    let rest = rest[name.len()..].trim_start();
    rest.starts_with('(') || rest.starts_with('<') || rest.starts_with("?(")
}

fn parse_ts_method_declaration(declaration: &str) -> TsMethodParse {
    let declaration = declaration.trim();
    let mut rest = declaration;
    let mut retained_modifiers = Vec::new();
    loop {
        let Some(modifier) = ident_prefix(rest) else {
            return TsMethodParse::NotMethod;
        };
        match modifier {
            "private" | "protected" | "get" | "set" => return TsMethodParse::NotMethod,
            "async" | "static" => {
                retained_modifiers.push(modifier);
                rest = rest[modifier.len()..].trim_start();
            }
            "public" | "override" | "abstract" | "declare" => {
                rest = rest[modifier.len()..].trim_start();
            }
            _ => break,
        }
    }
    let Some(method) = ident_prefix(rest) else {
        return TsMethodParse::NotMethod;
    };
    if matches!(
        method,
        "constructor" | "if" | "for" | "while" | "switch" | "catch" | "function" | "return" | "new"
    ) {
        return TsMethodParse::NotMethod;
    }
    let method_start = declaration.len() - rest.len();
    let mut tail = rest[method.len()..].trim_start();
    if let Some(after_optional) = tail.strip_prefix('?') {
        tail = after_optional.trim_start();
    }
    if tail.starts_with('<') {
        let Some(generic_end) = matching_ts_delimiter(tail, 0, '<', '>') else {
            return TsMethodParse::Incomplete;
        };
        tail = tail[generic_end + 1..].trim_start();
    }
    if !tail.starts_with('(') {
        return TsMethodParse::NotMethod;
    }
    let Some(params_end) = matching_ts_delimiter(tail, 0, '(', ')') else {
        return TsMethodParse::Incomplete;
    };
    let after_params = tail[params_end + 1..].trim_start();
    if after_params.is_empty() {
        return TsMethodParse::Incomplete;
    }

    let (return_type, signature_end) = if let Some(after_colon) = after_params.strip_prefix(':') {
        let after_colon = after_colon.trim_start();
        let Some(type_end) = ts_return_type_end(after_colon) else {
            return TsMethodParse::Incomplete;
        };
        let return_type = after_colon[..type_end].trim();
        if return_type.is_empty() {
            return TsMethodParse::NotMethod;
        }
        let end = declaration.len() - after_colon.len() + type_end;
        (Some(normalize_ts_type(return_type)), end)
    } else if after_params.starts_with('{') || after_params.starts_with(';') {
        (None, declaration.len() - after_params.len())
    } else {
        return TsMethodParse::NotMethod;
    };

    let mut public_signature = String::new();
    if !retained_modifiers.is_empty() {
        public_signature.push_str(&retained_modifiers.join(" "));
        public_signature.push(' ');
    }
    public_signature.push_str(declaration[method_start..signature_end].trim());
    TsMethodParse::Method(TsParsedMethod {
        name: method.to_string(),
        return_type,
        signature: normalize_ts_signature(&public_signature),
    })
}

fn matching_ts_delimiter(value: &str, open_idx: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0_u32;
    let mut quote = None;
    let mut escape = false;
    for (idx, ch) in value.char_indices().filter(|(idx, _)| *idx >= open_idx) {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            ch if ch == open => depth += 1,
            ch if ch == close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn ts_return_type_end(value: &str) -> Option<usize> {
    let mut quote = None;
    let mut escape = false;
    let mut angle_depth = 0_u32;
    let mut square_depth = 0_u32;
    let mut paren_depth = 0_u32;
    let mut brace_depth = 0_u32;
    for (idx, ch) in value.char_indices() {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '[' => square_depth += 1,
            ']' if square_depth > 0 => square_depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            '{' if angle_depth == 0 && square_depth == 0 && paren_depth == 0 => {
                if brace_depth == 0 && ts_brace_starts_method_body(&value[..idx]) {
                    return Some(idx);
                }
                brace_depth += 1;
            }
            '}' if brace_depth > 0 => brace_depth -= 1,
            ';' if angle_depth == 0
                && square_depth == 0
                && paren_depth == 0
                && brace_depth == 0 =>
            {
                return Some(idx);
            }
            _ => {}
        }
    }
    None
}

fn ts_brace_starts_method_body(type_prefix: &str) -> bool {
    let type_prefix = type_prefix.trim_end();
    if type_prefix.is_empty() || type_prefix.ends_with("=>") {
        return false;
    }
    !type_prefix
        .chars()
        .next_back()
        .is_some_and(|ch| matches!(ch, '|' | '&' | '?' | ':' | ',' | '=' | '(' | '[' | '<'))
}

fn parse_interface_property(line: &str) -> Option<(String, TsInterfaceProperty)> {
    if line.is_empty() || line.starts_with("//") || line.starts_with('*') || line.starts_with('[') {
        return None;
    }
    let line = line.trim_end_matches(';').trim_end_matches(',').trim();
    let (left, right) = split_property_decl(line)?;
    let left = left.trim();
    let ty = normalize_ts_type(right.trim());
    if ty.is_empty() {
        return None;
    }
    let (name, optional) = property_name_and_optional(left)?;
    let nullable = ts_type_contains_null(&ty);
    Some((
        name,
        TsInterfaceProperty {
            optional,
            nullable,
            ty,
        },
    ))
}

fn split_property_decl(line: &str) -> Option<(&str, &str)> {
    let mut quote = None;
    for (idx, ch) in line.char_indices() {
        match (quote, ch) {
            (Some(active), c) if c == active => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, ':') => return Some((&line[..idx], &line[idx + 1..])),
            _ => {}
        }
    }
    None
}

fn property_name_and_optional(left: &str) -> Option<(String, bool)> {
    let optional = left.ends_with('?');
    let name = left.trim_end_matches('?').trim();
    if name.is_empty() {
        return None;
    }
    let name = if (name.starts_with('"') && name.ends_with('"'))
        || (name.starts_with('\'') && name.ends_with('\''))
    {
        name[1..name.len() - 1].to_string()
    } else {
        name.to_string()
    };
    Some((name, optional))
}

fn normalize_ts_type(ty: &str) -> String {
    normalize_ts_tokens(ty)
}

fn normalize_ts_signature(signature: &str) -> String {
    let signature = signature
        .trim()
        .trim_end_matches(';')
        .trim_end_matches(',')
        .trim();
    normalize_ts_tokens(signature)
}

fn rename_ts_identifiers(value: &str, renames: &BTreeMap<String, String>) -> String {
    if renames.is_empty() {
        return value.to_string();
    }
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    let mut previous_token: Option<String> = None;
    while let Some(ch) = chars.next() {
        if matches!(ch, '\'' | '"' | '`') {
            let mut literal = String::from(ch);
            let quote = ch;
            let mut escaped = false;
            for next in chars.by_ref() {
                literal.push(next);
                if escaped {
                    escaped = false;
                } else if next == '\\' {
                    escaped = true;
                } else if next == quote {
                    break;
                }
            }
            output.push_str(&literal);
            previous_token = Some(literal);
            continue;
        }
        if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let mut identifier = String::from(ch);
            while chars
                .peek()
                .is_some_and(|next| next.is_alphanumeric() || *next == '_' || *next == '$')
            {
                if let Some(next) = chars.next() {
                    identifier.push(next);
                }
            }
            let replacement =
                if ts_identifier_is_declaration_key(previous_token.as_deref(), chars.clone()) {
                    &identifier
                } else {
                    renames.get(&identifier).unwrap_or(&identifier)
                };
            output.push_str(replacement);
            previous_token = Some(identifier);
        } else {
            output.push(ch);
            if !ch.is_whitespace() {
                previous_token = Some(ch.to_string());
            }
        }
    }
    normalize_ts_tokens(&output)
}

fn ts_identifier_is_declaration_key(
    previous: Option<&str>,
    remaining: std::iter::Peekable<std::str::Chars<'_>>,
) -> bool {
    let following: Vec<char> = remaining.filter(|ch| !ch.is_whitespace()).take(2).collect();
    match following.as_slice() {
        [':', ..] | ['?', ':'] => true,
        ['(', ..] if matches!(previous, Some("{" | ";" | ",")) => true,
        ['=', ..] if !matches!(previous, Some("type" | "const")) => true,
        _ => false,
    }
}

fn normalize_ts_tokens(value: &str) -> String {
    let mut tokens = Vec::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_alphanumeric() || ch == '_' || ch == '$' {
            let mut token = String::from(ch);
            while chars
                .peek()
                .is_some_and(|next| next.is_alphanumeric() || *next == '_' || *next == '$')
            {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            tokens.push(token);
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            let mut token = String::from(ch);
            let quote = ch;
            let mut escape = false;
            for next in chars.by_ref() {
                token.push(next);
                if escape {
                    escape = false;
                } else if next == '\\' {
                    escape = true;
                } else if next == quote {
                    break;
                }
            }
            tokens.push(token);
            continue;
        }
        let mut token = String::from(ch);
        if ch == '.' && chars.peek() == Some(&'.') {
            if let Some(next) = chars.next() {
                token.push(next);
            }
            if chars.peek() == Some(&'.') {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
        } else if ch == '=' && chars.peek() == Some(&'>') {
            if let Some(next) = chars.next() {
                token.push(next);
            }
        }
        tokens.push(token);
    }

    let mut normalized: Vec<String> = Vec::with_capacity(tokens.len());
    for token in tokens {
        if matches!(token.as_str(), ")" | "]" | "}")
            && normalized.last().is_some_and(|previous| previous == ",")
        {
            normalized.pop();
        }
        if normalized
            .last()
            .is_some_and(|previous| ts_tokens_need_space(previous, &token))
        {
            normalized.push(" ".to_string());
        }
        normalized.push(token);
    }
    normalized.concat()
}

fn ts_token_is_atom(token: &str) -> bool {
    token.chars().next().is_some_and(|ch| {
        ch.is_alphanumeric() || ch == '_' || ch == '$' || matches!(ch, '\'' | '"' | '`')
    })
}

fn ts_tokens_need_space(previous: &str, current: &str) -> bool {
    if ts_token_is_atom(previous) && ts_token_is_atom(current) {
        return true;
    }
    if matches!(previous, "," | ":" | "|" | "&" | "=" | "=>")
        && !matches!(current, ")" | "]" | "}" | "," | ";")
    {
        return true;
    }
    if previous == "{" && current != "}" {
        return true;
    }
    if current == "}" && previous != "{" {
        return true;
    }
    matches!(current, "|" | "&" | "=" | "=>")
}

fn ts_type_contains_null(ty: &str) -> bool {
    ty.split('|').any(|part| part.trim() == "null")
}

fn ident_prefix(value: &str) -> Option<&str> {
    let end = value
        .char_indices()
        .take_while(|(i, ch)| {
            if *i == 0 {
                ch.is_ascii_alphabetic() || *ch == '_' || *ch == '$'
            } else {
                ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$'
            }
        })
        .map(|(i, ch)| i + ch.len_utf8())
        .last()?;
    Some(&value[..end])
}

struct TsNamedReexport {
    source: String,
    exported: String,
    type_only: bool,
}

enum TsReexport {
    Named {
        exports: Vec<TsNamedReexport>,
        module: Option<String>,
    },
    Star {
        module: String,
        namespace: Option<String>,
        type_only: bool,
    },
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TsExportOrigin {
    file: PathBuf,
    symbol: String,
}

#[derive(Clone)]
struct TsResolvedExport {
    kind: TsExportKind,
    origins: BTreeSet<TsExportOrigin>,
}

fn ts_reexports(text: &str) -> Vec<TsReexport> {
    ts_reexport_statements(text)
        .iter()
        .filter_map(|statement| parse_ts_reexport(statement))
        .collect()
}

fn ts_reexport_statements(text: &str) -> Vec<String> {
    let code = sanitize_typescript(text, false);
    let lines: Vec<_> = code.lines().collect();
    let mut statements = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if !is_ts_reexport_start(line) {
            index += 1;
            continue;
        }
        let mut statement = line.to_string();
        if line.starts_with("export {") || line.starts_with("export type {") {
            while !statement.contains('}') && index + 1 < lines.len() {
                index += 1;
                statement.push(' ');
                statement.push_str(lines[index].trim());
            }
            if statement
                .split_once('}')
                .is_some_and(|(_, suffix)| suffix.trim().is_empty())
                && index + 1 < lines.len()
                && lines[index + 1].trim_start().starts_with("from ")
            {
                index += 1;
                statement.push(' ');
                statement.push_str(lines[index].trim());
            }
        }
        statements.push(statement);
        index += 1;
    }
    statements
}

fn is_ts_reexport_start(line: &str) -> bool {
    line.starts_with("export {")
        || line.starts_with("export type {")
        || line.starts_with("export *")
        || line.starts_with("export type *")
}

fn parse_ts_reexport(statement: &str) -> Option<TsReexport> {
    let mut rest = strip_ts_keyword(statement, "export")?;
    let mut type_only = false;
    if let Some(after_type) = strip_ts_keyword(rest, "type") {
        if after_type.starts_with('{') || after_type.starts_with('*') {
            type_only = true;
            rest = after_type;
        }
    }
    if let Some(after_star) = rest.strip_prefix('*') {
        let mut after_star = after_star.trim_start();
        let namespace = if let Some(after_as) = strip_ts_keyword(after_star, "as") {
            let name = ident_prefix(after_as)?.to_string();
            after_star = after_as[name.len()..].trim_start();
            Some(name)
        } else {
            None
        };
        let after_from = strip_ts_keyword(after_star, "from")?;
        return Some(TsReexport::Star {
            module: quoted_module_spec(after_from)?.to_string(),
            namespace,
            type_only,
        });
    }

    let after_open = rest.strip_prefix('{')?;
    let (list, after_close) = after_open.split_once('}')?;
    let module = strip_ts_keyword(after_close, "from")
        .and_then(quoted_module_spec)
        .map(str::to_string);
    let mut exports = Vec::new();
    for raw_export in list.split(',') {
        let mut export = raw_export.trim();
        if export.is_empty() {
            continue;
        }
        let mut item_type_only = type_only;
        if let Some(after_type) = strip_ts_keyword(export, "type") {
            item_type_only = true;
            export = after_type;
        }
        let source = ident_prefix(export)?;
        let after_source = export[source.len()..].trim_start();
        let exported = if let Some(after_as) = strip_ts_keyword(after_source, "as") {
            ident_prefix(after_as)?
        } else {
            source
        };
        exports.push(TsNamedReexport {
            source: source.to_string(),
            exported: exported.to_string(),
            type_only: item_type_only,
        });
    }
    Some(TsReexport::Named { exports, module })
}

#[expect(
    clippy::too_many_lines,
    reason = "recursive re-export resolution keeps direct, star, and named provenance transitions together"
)]
fn resolved_ts_exports_from_file(
    dir: &Path,
    rel: &Path,
    cache: &mut BTreeMap<PathBuf, BTreeMap<String, TsResolvedExport>>,
    stack: &mut Vec<PathBuf>,
) -> Result<BTreeMap<String, TsResolvedExport>, CoreError> {
    if let Some(exports) = cache.get(rel) {
        return Ok(exports.clone());
    }
    if stack.iter().any(|path| path == rel) {
        return Ok(BTreeMap::new());
    }
    stack.push(rel.to_path_buf());

    let text = read_to_string(dir.join(rel))?;
    let parsed = parse_ts_file(&text);
    let mut exports = BTreeMap::new();
    for (name, kind) in &parsed.exports {
        let origins = parsed
            .export_origins
            .get(name)
            .into_iter()
            .flatten()
            .map(|symbol| TsExportOrigin {
                file: rel.to_path_buf(),
                symbol: symbol.clone(),
            })
            .collect();
        add_resolved_ts_export(&mut exports, name, *kind, origins);
    }

    let base = rel.parent().unwrap_or_else(|| Path::new(""));
    for reexport in ts_reexports(&text) {
        match reexport {
            TsReexport::Star {
                module,
                namespace,
                type_only,
            } => {
                if let Some(namespace) = namespace {
                    add_resolved_ts_export(
                        &mut exports,
                        &namespace,
                        if type_only {
                            TsExportKind::Type
                        } else {
                            TsExportKind::Value
                        },
                        BTreeSet::new(),
                    );
                    continue;
                }
                let Some(target) = resolve_ts_reexport_target(dir, base, rel, &module)? else {
                    continue;
                };
                for (name, resolved) in resolved_ts_exports_from_file(dir, &target, cache, stack)? {
                    if name == "default" {
                        continue;
                    }
                    add_resolved_ts_export(
                        &mut exports,
                        &name,
                        if type_only {
                            TsExportKind::Type
                        } else {
                            resolved.kind
                        },
                        resolved.origins,
                    );
                }
            }
            TsReexport::Named {
                exports: named,
                module,
            } => {
                let (source_exports, fallback_file) = if let Some(module) = module {
                    if let Some(target) = resolve_ts_reexport_target(dir, base, rel, &module)? {
                        (
                            resolved_ts_exports_from_file(dir, &target, cache, stack)?,
                            Some(target),
                        )
                    } else {
                        (BTreeMap::new(), None)
                    }
                } else {
                    (exports.clone(), Some(rel.to_path_buf()))
                };
                for named_export in named {
                    let source = source_exports.get(&named_export.source);
                    let kind = if named_export.type_only {
                        TsExportKind::Type
                    } else {
                        source.map_or_else(
                            || {
                                parsed
                                    .declaration_kinds
                                    .get(&named_export.source)
                                    .copied()
                                    .unwrap_or(TsExportKind::Value)
                            },
                            |resolved| resolved.kind,
                        )
                    };
                    let origins = source.map_or_else(
                        || {
                            fallback_file
                                .iter()
                                .map(|file| TsExportOrigin {
                                    file: file.clone(),
                                    symbol: named_export.source.clone(),
                                })
                                .collect()
                        },
                        |resolved| resolved.origins.clone(),
                    );
                    add_resolved_ts_export(&mut exports, &named_export.exported, kind, origins);
                }
            }
        }
    }

    stack.pop();
    cache.insert(rel.to_path_buf(), exports.clone());
    Ok(exports)
}

fn add_resolved_ts_export(
    into: &mut BTreeMap<String, TsResolvedExport>,
    name: &str,
    kind: TsExportKind,
    origins: BTreeSet<TsExportOrigin>,
) {
    into.entry(name.to_string())
        .and_modify(|existing| {
            if existing.kind != kind {
                existing.kind = TsExportKind::Both;
            }
            existing.origins.extend(origins.clone());
        })
        .or_insert(TsResolvedExport { kind, origins });
}

fn public_ts_export_bindings(
    dir: &Path,
    files: &[PathBuf],
) -> Result<BTreeMap<TsExportOrigin, BTreeSet<String>>, CoreError> {
    let mut cache = BTreeMap::new();
    let mut bindings = BTreeMap::new();
    if let Some(index) = ts_root_index(files) {
        let exports = resolved_ts_exports_from_file(dir, index, &mut cache, &mut Vec::new())?;
        add_ts_public_bindings(&mut bindings, &exports);
    } else {
        for file in files {
            let exports = resolved_ts_exports_from_file(dir, file, &mut cache, &mut Vec::new())?;
            add_ts_public_bindings(&mut bindings, &exports);
        }
    }
    for model_file in files.iter().filter(|path| is_model_file(path)) {
        let exports = resolved_ts_exports_from_file(dir, model_file, &mut cache, &mut Vec::new())?;
        add_ts_public_bindings(&mut bindings, &exports);
    }
    Ok(bindings)
}

fn add_ts_public_bindings(
    into: &mut BTreeMap<TsExportOrigin, BTreeSet<String>>,
    exports: &BTreeMap<String, TsResolvedExport>,
) {
    for (public_name, resolved) in exports {
        for origin in &resolved.origins {
            into.entry(origin.clone())
                .or_default()
                .insert(public_name.clone());
        }
    }
}

fn unambiguous_ts_public_identifier_renames(
    bindings: &BTreeMap<TsExportOrigin, BTreeSet<String>>,
) -> BTreeMap<String, String> {
    let mut candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (origin, public_names) in bindings {
        candidates
            .entry(origin.symbol.clone())
            .or_default()
            .extend(public_names.iter().cloned());
    }
    candidates
        .into_iter()
        .filter_map(|(origin, public_names)| {
            (public_names.len() == 1).then(|| {
                let public_name = public_names.into_iter().next().unwrap_or_default();
                (origin, public_name)
            })
        })
        .collect()
}

fn add_export(into: &mut BTreeMap<String, TsExportKind>, name: &str, kind: TsExportKind) {
    into.entry(name.to_string())
        .and_modify(|existing| {
            if *existing != kind {
                *existing = TsExportKind::Both;
            }
        })
        .or_insert(kind);
}

fn merge_exports(
    into: &mut BTreeMap<String, TsExportKind>,
    exports: &BTreeMap<String, TsExportKind>,
) {
    for (name, kind) in exports {
        add_export(into, name, *kind);
    }
}

fn merge_interface_properties(
    into: &mut BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
    properties: BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
) {
    for (interface, fields) in properties {
        into.entry(interface).or_default().extend(fields);
    }
}

fn merge_ts_declaration_shapes(
    into: &mut BTreeMap<String, Vec<String>>,
    declarations: BTreeMap<String, Vec<String>>,
) {
    for (symbol, shapes) in declarations {
        for shape in shapes {
            add_ts_declaration_shape(into, &symbol, shape);
        }
    }
}

fn add_ts_declaration_shape(into: &mut BTreeMap<String, Vec<String>>, symbol: &str, shape: String) {
    let shapes = into.entry(symbol.to_string()).or_default();
    if !shapes.contains(&shape) {
        shapes.push(shape);
        shapes.sort();
    }
}

fn extract_root_exports(
    dir: &Path,
    files: &[PathBuf],
    all_exports: &BTreeMap<String, TsExportKind>,
) -> Result<BTreeMap<String, TsExportKind>, CoreError> {
    let Some(index) = ts_root_index(files) else {
        return Ok(all_exports.clone());
    };
    let mut cache = BTreeMap::new();
    let mut stack = Vec::new();
    exports_from_file(dir, index, &mut cache, &mut stack)
}

fn ts_root_index(files: &[PathBuf]) -> Option<&Path> {
    ["index.ts", "index.d.ts", "src/index.ts", "src/index.d.ts"]
        .into_iter()
        .find_map(|candidate| {
            files
                .iter()
                .find(|path| path.as_path() == Path::new(candidate))
                .map(PathBuf::as_path)
        })
}

fn exports_from_file(
    dir: &Path,
    rel: &Path,
    cache: &mut BTreeMap<PathBuf, BTreeMap<String, TsExportKind>>,
    stack: &mut Vec<PathBuf>,
) -> Result<BTreeMap<String, TsExportKind>, CoreError> {
    if let Some(exports) = cache.get(rel) {
        return Ok(exports.clone());
    }
    if stack.iter().any(|path| path == rel) {
        return Ok(BTreeMap::new());
    }
    stack.push(rel.to_path_buf());

    let text = read_to_string(dir.join(rel))?;
    let parsed = parse_ts_file(&text);
    let direct_exports = parsed.exports;
    let mut exports = direct_exports.clone();
    let base = rel.parent().unwrap_or_else(|| Path::new(""));
    for reexport in ts_reexports(&text) {
        match reexport {
            TsReexport::Star {
                module,
                namespace,
                type_only,
            } => {
                if let Some(namespace) = namespace {
                    add_export(
                        &mut exports,
                        &namespace,
                        if type_only {
                            TsExportKind::Type
                        } else {
                            TsExportKind::Value
                        },
                    );
                    continue;
                }
                let Some(target) = resolve_ts_reexport_target(dir, base, rel, &module)? else {
                    continue;
                };
                let nested = exports_from_file(dir, &target, cache, stack)?;
                for (name, kind) in nested {
                    if name == "default" {
                        continue;
                    }
                    add_export(
                        &mut exports,
                        &name,
                        if type_only { TsExportKind::Type } else { kind },
                    );
                }
            }
            TsReexport::Named {
                exports: named,
                module,
            } => {
                let source_exports = if let Some(module) = module {
                    resolve_ts_reexport_target(dir, base, rel, &module)?
                        .map(|target| exports_from_file(dir, &target, cache, stack))
                        .transpose()?
                        .unwrap_or_default()
                } else {
                    direct_exports.clone()
                };
                for named_export in named {
                    let kind = if named_export.type_only {
                        TsExportKind::Type
                    } else {
                        source_exports
                            .get(&named_export.source)
                            .copied()
                            .or_else(|| parsed.declaration_kinds.get(&named_export.source).copied())
                            .unwrap_or(TsExportKind::Value)
                    };
                    add_export(&mut exports, &named_export.exported, kind);
                }
            }
        }
    }

    stack.pop();
    cache.insert(rel.to_path_buf(), exports.clone());
    Ok(exports)
}

fn resolve_ts_reexport_target(
    dir: &Path,
    base: &Path,
    source_file: &Path,
    module: &str,
) -> Result<Option<PathBuf>, CoreError> {
    let target = resolve_ts_module(dir, base, module);
    if module.starts_with('.') && target.is_none() {
        return Err(CoreError::Workspace {
            message: format!(
                "TypeScript re-export in {} references missing module {module:?}",
                source_file.display()
            ),
        });
    }
    Ok(target)
}

fn quoted_module_spec(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &value[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

fn resolve_ts_module(dir: &Path, base: &Path, spec: &str) -> Option<PathBuf> {
    if !spec.starts_with('.') {
        return None;
    }
    let joined = normalize_relative_path(&base.join(spec));
    [
        joined.with_extension("ts"),
        joined.with_extension("d.ts"),
        joined.with_extension("tsx"),
        joined.join("index.ts"),
        joined.join("index.d.ts"),
        joined.clone(),
    ]
    .into_iter()
    .find(|candidate| dir.join(candidate).is_file())
}

fn normalize_relative_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {}
        }
    }
    out
}

fn collect_ts_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CoreError> {
    let entries = std::fs::read_dir(dir).map_err(|err| CoreError::Workspace {
        message: format!("failed to read TypeScript SDK dir {}: {err}", dir.display()),
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| CoreError::Workspace {
            message: format!(
                "failed to read TypeScript SDK dir entry {}: {err}",
                dir.display()
            ),
        })?;
        let path = entry.path();
        let file_type = sdk_entry_file_type(&entry, "TypeScript SDK")?;
        if file_type.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("node_modules") {
                continue;
            }
            collect_ts_files(root, &path, out)?;
        } else if file_type.is_file()
            && matches!(
                path.extension().and_then(|ext| ext.to_str()),
                Some("ts" | "tsx")
            )
        {
            let rel = path
                .strip_prefix(root)
                .map_err(|err| CoreError::Workspace {
                    message: format!(
                        "failed to relativize TypeScript file {}: {err}",
                        path.display()
                    ),
                })?
                .to_path_buf();
            out.push(rel);
        }
    }
    Ok(())
}

fn read_to_string(path: impl AsRef<Path>) -> Result<String, CoreError> {
    let path = path.as_ref();
    std::fs::read_to_string(path).map_err(|err| CoreError::Workspace {
        message: format!("failed to read {}: {err}", path.display()),
    })
}

fn sdk_entry_file_type(
    entry: &std::fs::DirEntry,
    surface: &str,
) -> Result<std::fs::FileType, CoreError> {
    let path = entry.path();
    let file_type = entry.file_type().map_err(|error| CoreError::Workspace {
        message: format!(
            "failed to inspect {surface} path {}: {error}",
            path.display()
        ),
    })?;
    if file_type.is_symlink() {
        return Err(CoreError::Workspace {
            message: format!(
                "{surface} surface traversal does not follow symbolic link {}",
                path.display()
            ),
        });
    }
    Ok(file_type)
}

fn is_model_file(path: &Path) -> bool {
    path == Path::new("models.ts")
        || path
            .components()
            .next()
            .and_then(|component| match component {
                std::path::Component::Normal(name) => name.to_str(),
                _ => None,
            })
            == Some("models")
}

fn package_entry_points(dir: &Path) -> Result<BTreeMap<String, String>, CoreError> {
    let path = dir.join("package.json");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = read_to_string(path)?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| CoreError::Workspace {
            message: format!("failed to parse package.json for TypeScript SDK surface: {err}"),
        })?;
    let mut out = BTreeMap::new();
    for key in ["main", "module", "types", "exports"] {
        if let Some(value) = value.get(key) {
            out.insert(key.to_string(), value.to_string());
        }
    }
    Ok(out)
}

#[derive(Default)]
struct ParsedPythonFile {
    models: BTreeMap<String, PythonModelSurface>,
    exceptions: Vec<String>,
    aliases: BTreeMap<String, String>,
}

struct PythonClassState {
    name: String,
    indent: usize,
    member_indent: Option<usize>,
    is_model: bool,
    is_exception: bool,
    typed_dict_total: Option<bool>,
    model: PythonModelSurface,
}

fn parse_python_file(text: &str, model_file: bool) -> ParsedPythonFile {
    let lines: Vec<&str> = text.lines().collect();
    let mut parsed = ParsedPythonFile::default();
    let mut current: Option<PythonClassState> = None;
    let mut pending_decorators = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let raw = lines[index];
        let trimmed = strip_python_comment(raw).trim();
        let indent = python_indent(raw);

        if let Some(state) = current.as_ref() {
            if !trimmed.is_empty() && indent <= state.indent {
                finish_python_class(&mut parsed, current.take());
                continue;
            }
        }

        if current.is_none() && indent == 0 {
            if trimmed.starts_with('@') {
                pending_decorators.push(trimmed.to_string());
                index += 1;
                continue;
            }
            if trimmed.starts_with("class ") {
                let (signature, last) = collect_python_class_signature(&lines, index);
                if let Some((name, bases)) = python_class_decl(&signature) {
                    if !name.starts_with('_') {
                        current = Some(start_python_class(
                            name,
                            &bases,
                            indent,
                            model_file,
                            &pending_decorators,
                        ));
                    }
                    pending_decorators.clear();
                    index = last + 1;
                    continue;
                }
            }
            if !trimmed.is_empty() {
                pending_decorators.clear();
            }
            if let Some((alias, target)) = python_type_alias(trimmed) {
                parsed.aliases.insert(alias, target);
            }
        }

        if let Some(state) = current.as_mut() {
            if trimmed.is_empty() || indent <= state.indent {
                index += 1;
                continue;
            }
            let member_indent = *state.member_indent.get_or_insert(indent);
            if indent == member_indent {
                if python_function_start(trimmed).is_some() {
                    let (signature, last) = collect_python_function_signature(&lines, index);
                    if let Some(method) = python_function_start(signature.trim()) {
                        match method.as_str() {
                            "__init__" => {
                                state.model.constructor =
                                    Some(normalize_python_constructor(&signature));
                            }
                            "from_dict" => state.model.has_from_dict = true,
                            "to_dict" => state.model.has_to_dict = true,
                            _ => {}
                        }
                    }
                    index = last + 1;
                    continue;
                }
                if python_top_level_delimiter(trimmed, ':').is_some() {
                    let (declaration, last) = collect_python_field_declaration(&lines, index);
                    if let Some((field_name, mut field)) = parse_python_model_field(&declaration) {
                        if !state.is_exception {
                            state.is_model = true;
                        }
                        if state.typed_dict_total == Some(false)
                            && python_outer_type_name(&field.ty) != Some("Required")
                        {
                            field.required = false;
                        }
                        if let Some(alias) = &field.alias {
                            parsed
                                .aliases
                                .insert(format!("{}.{}", state.name, field_name), alias.clone());
                        }
                        state.model.fields.insert(field_name, field);
                    }
                    index = last + 1;
                    continue;
                }
            }
        }
        index += 1;
    }
    finish_python_class(&mut parsed, current);
    parsed.exceptions.sort();
    parsed.exceptions.dedup();
    parsed
}

fn start_python_class(
    name: String,
    bases: &str,
    indent: usize,
    model_file: bool,
    decorators: &[String],
) -> PythonClassState {
    let is_exception = python_exception_bases(bases);
    PythonClassState {
        name,
        indent,
        member_indent: None,
        is_model: !is_exception
            && (model_file || python_model_bases(bases) || python_model_decorators(decorators)),
        is_exception,
        typed_dict_total: python_typed_dict_total(bases),
        model: PythonModelSurface::default(),
    }
}

fn collect_python_class_signature(lines: &[&str], start: usize) -> (String, usize) {
    let mut signature = String::new();
    let mut index = start;
    while index < lines.len() {
        if !signature.is_empty() {
            signature.push(' ');
        }
        signature.push_str(strip_python_comment(lines[index]).trim());
        if python_top_level_delimiter(&signature, ':').is_some() {
            break;
        }
        index += 1;
    }
    (signature, index.min(lines.len().saturating_sub(1)))
}

fn collect_python_field_declaration(lines: &[&str], start: usize) -> (String, usize) {
    let mut declaration = strip_python_comment(lines[start]).trim().to_string();
    let mut index = start;
    while python_has_unclosed_delimiters(&declaration) && index + 1 < lines.len() {
        index += 1;
        declaration.push(' ');
        declaration.push_str(strip_python_comment(lines[index]).trim());
    }
    (declaration, index)
}

fn python_has_unclosed_delimiters(value: &str) -> bool {
    let mut round = 0_u32;
    let mut square = 0_u32;
    let mut curly = 0_u32;
    let mut quote = None;
    let mut escape = false;
    for ch in value.chars() {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => round += 1,
            ')' => round = round.saturating_sub(1),
            '[' => square += 1,
            ']' => square = square.saturating_sub(1),
            '{' => curly += 1,
            '}' => curly = curly.saturating_sub(1),
            _ => {}
        }
    }
    round > 0 || square > 0 || curly > 0
}

fn strip_python_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escape = false;
    for (index, ch) in line.char_indices() {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '#' => return &line[..index],
            _ => {}
        }
    }
    line
}

fn finish_python_class(parsed: &mut ParsedPythonFile, state: Option<PythonClassState>) {
    let Some(state) = state else {
        return;
    };
    if state.is_exception {
        parsed.exceptions.push(state.name.clone());
    }
    if state.is_model {
        parsed.models.insert(state.name, state.model);
    }
}

fn python_indent(line: &str) -> usize {
    line.chars()
        .take_while(char::is_ascii_whitespace)
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

fn python_class_decl(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("class ")?;
    let name = ident_prefix(rest)?.to_string();
    let mut after = rest[name.len()..].trim_start();
    if after.starts_with('[') {
        let close = matching_python_delimiter(after, 0, '[', ']')?;
        after = after[close + 1..].trim_start();
    }
    let bases = if after.starts_with('(') {
        let close = matching_python_delimiter(after, 0, '(', ')')?;
        let bases = after[1..close].to_string();
        after = after[close + 1..].trim_start();
        bases
    } else {
        String::new()
    };
    (after == ":").then_some((name, bases))
}

fn python_exception_bases(bases: &str) -> bool {
    split_python_top_level(bases, ',').iter().any(|base| {
        let name = base.trim().rsplit('.').next().unwrap_or_default().trim();
        name == "BaseException" || name.ends_with("Exception") || name.ends_with("Error")
    })
}

fn python_model_bases(bases: &str) -> bool {
    split_python_top_level(bases, ',').iter().any(|base| {
        let base = base.trim();
        if base.contains('=') {
            return false;
        }
        let name = base
            .split_once('[')
            .map_or(base, |(name, _)| name)
            .rsplit('.')
            .next()
            .unwrap_or_default();
        matches!(
            name,
            "BaseModel" | "BaseSettings" | "RootModel" | "TypedDict"
        )
    })
}

fn python_typed_dict_total(bases: &str) -> Option<bool> {
    let parts = split_python_top_level(bases, ',');
    let typed_dict = parts.iter().any(|base| {
        let base = base.trim();
        let name = base
            .split_once('[')
            .map_or(base, |(name, _)| name)
            .rsplit('.')
            .next()
            .unwrap_or_default();
        name == "TypedDict"
    });
    typed_dict.then(|| {
        parts
            .iter()
            .find_map(|part| {
                let equal = python_top_level_delimiter(part, '=')?;
                (part[..equal].trim() == "total")
                    .then(|| !matches!(part[equal + 1..].trim(), "False" | "false" | "0"))
            })
            .unwrap_or(true)
    })
}

fn python_model_decorators(decorators: &[String]) -> bool {
    decorators.iter().any(|decorator| {
        let name = decorator
            .trim_start_matches('@')
            .split_once('(')
            .map_or_else(|| decorator.trim_start_matches('@'), |(name, _)| name)
            .trim();
        let leaf = name.rsplit('.').next().unwrap_or_default();
        leaf == "dataclass"
            || leaf == "define"
            || leaf == "frozen"
            || name == "attr.s"
            || name == "attrs.mutable"
    })
}

fn python_function_start(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("async def ")
        .or_else(|| line.strip_prefix("def "))?;
    let name = ident_prefix(rest)?;
    rest[name.len()..]
        .trim_start()
        .starts_with('(')
        .then(|| name.to_string())
}

fn collect_python_function_signature(lines: &[&str], start: usize) -> (String, usize) {
    let mut signature = String::new();
    let mut paren_depth = 0_i32;
    let mut quote = None;
    let mut escape = false;
    let mut index = start;
    while index < lines.len() {
        if !signature.is_empty() {
            signature.push(' ');
        }
        let part = strip_python_comment(lines[index]).trim();
        signature.push_str(part);
        for ch in part.chars() {
            if let Some(active) = quote {
                if escape {
                    escape = false;
                } else if ch == '\\' {
                    escape = true;
                } else if ch == active {
                    quote = None;
                }
                continue;
            }
            match ch {
                '\'' | '"' => quote = Some(ch),
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                _ => {}
            }
        }
        if paren_depth <= 0 && part.ends_with(':') {
            break;
        }
        index += 1;
    }
    (signature, index.min(lines.len().saturating_sub(1)))
}

fn parse_python_model_field(line: &str) -> Option<(String, PythonFieldSurface)> {
    if line.starts_with('@') || line.starts_with("def ") || line.starts_with("async def ") {
        return None;
    }
    let colon = python_top_level_delimiter(line, ':')?;
    let name = line[..colon].trim();
    if name.starts_with('_') || ident_prefix(name) != Some(name) {
        return None;
    }
    let declaration = line[colon + 1..].trim();
    let equal = python_top_level_delimiter(declaration, '=');
    let (annotation, default) = equal.map_or((declaration, None), |index| {
        (
            declaration[..index].trim(),
            Some(declaration[index + 1..].trim()),
        )
    });
    let ty = canonical_python_type(annotation);
    if ty.starts_with("ClassVar[") || ty == "ClassVar" {
        return None;
    }
    let required = if python_outer_type_name(&ty) == Some("NotRequired") {
        false
    } else {
        default.is_none_or(python_default_is_required)
    };
    let alias = default
        .and_then(python_field_alias)
        .or_else(|| python_annotation_alias(annotation));
    Some((
        name.to_string(),
        PythonFieldSurface {
            required,
            nullable: python_type_is_nullable(&ty),
            ty,
            alias,
        },
    ))
}

fn python_default_is_required(default: &str) -> bool {
    let default = default.trim();
    if default == "..." {
        return true;
    }
    if let Some(args) = python_call_args(default, "Field") {
        if let Some(factory) = python_keyword_argument(args, "default_factory") {
            return factory.trim() == "None" || python_missing_default(factory);
        }
        if let Some(value) = python_keyword_argument(args, "default") {
            return python_missing_default(value);
        }
        return python_first_positional_argument(args).is_none_or(python_missing_default);
    }
    for call in ["field", "ib", "attrib"] {
        let Some(args) = python_call_args(default, call) else {
            continue;
        };
        if let Some(factory) = python_keyword_argument(args, "default_factory")
            .or_else(|| python_keyword_argument(args, "factory"))
        {
            return factory.trim() == "None" || python_missing_default(factory);
        }
        if let Some(value) = python_keyword_argument(args, "default") {
            return python_missing_default(value);
        }
        return python_first_positional_argument(args).is_none_or(python_missing_default);
    }
    false
}

fn python_call_args<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let value = value.trim();
    let open = value.find('(')?;
    let callable = value[..open].trim();
    if callable.rsplit('.').next()? != name {
        return None;
    }
    let close = matching_python_delimiter(value, open, '(', ')')?;
    value[close + 1..]
        .trim()
        .is_empty()
        .then_some(&value[open + 1..close])
}

fn python_missing_default(value: &str) -> bool {
    matches!(
        value.trim(),
        "..." | "MISSING" | "dataclasses.MISSING" | "PydanticUndefined" | "Undefined"
    )
}

fn python_first_positional_argument(args: &str) -> Option<&str> {
    split_python_top_level(args, ',')
        .into_iter()
        .find_map(|part| {
            let part = part.trim();
            (!part.is_empty() && python_top_level_delimiter(part, '=').is_none()).then_some(part)
        })
}

fn python_keyword_argument<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    split_python_top_level(args, ',')
        .into_iter()
        .find_map(|part| {
            let equal = python_top_level_delimiter(part, '=')?;
            (part[..equal].trim() == key).then(|| part[equal + 1..].trim())
        })
}

fn python_field_alias(default: &str) -> Option<String> {
    for call in ["Field", "field"] {
        let Some(args) = python_call_args(default, call) else {
            continue;
        };
        for key in ["alias", "validation_alias", "serialization_alias"] {
            if let Some(value) = python_keyword_string(args, key) {
                return Some(value);
            }
        }
    }
    None
}

fn python_annotation_alias(annotation: &str) -> Option<String> {
    let compact = python_compact_type(annotation);
    let (name, inner) = python_outer_generic(&compact)?;
    if canonical_python_type_head(name) != "Annotated" {
        return None;
    }
    split_python_top_level(inner, ',')
        .into_iter()
        .skip(1)
        .find_map(python_field_alias)
}

fn python_keyword_string(args: &str, key: &str) -> Option<String> {
    let rest = python_keyword_argument(args, key)?;
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let after = &rest[quote.len_utf8()..];
    let mut escaped = false;
    for (index, ch) in after.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return after[index + quote.len_utf8()..]
                .trim()
                .is_empty()
                .then(|| after[..index].to_string());
        }
    }
    None
}

fn normalize_python_constructor(signature: &str) -> String {
    let Some(open) = signature.find('(') else {
        return collapse_python_whitespace(signature);
    };
    let Some(close) = matching_python_delimiter(signature, open, '(', ')') else {
        return collapse_python_whitespace(signature);
    };
    let mut parameters = Vec::new();
    for parameter in split_python_top_level(&signature[open + 1..close], ',') {
        let parameter = parameter.trim();
        if parameter.is_empty() || parameter == "self" {
            continue;
        }
        if parameter == "/" || parameter == "*" {
            parameters.push(parameter.to_string());
            continue;
        }
        parameters.push(normalize_python_parameter(parameter));
    }
    format!("({})", parameters.join(", "))
}

fn normalize_python_parameter(parameter: &str) -> String {
    let equal = python_top_level_delimiter(parameter, '=');
    let (left, default) = equal.map_or((parameter.trim(), None), |index| {
        (
            parameter[..index].trim(),
            Some(parameter[index + 1..].trim()),
        )
    });
    let colon = python_top_level_delimiter(left, ':');
    let normalized = colon.map_or_else(
        || left.to_string(),
        |index| {
            format!(
                "{}: {}",
                left[..index].trim(),
                canonical_python_type(left[index + 1..].trim())
            )
        },
    );
    default.map_or(normalized.clone(), |default| {
        format!("{normalized} = {}", python_compact_type(default))
    })
}

fn canonical_python_type(annotation: &str) -> String {
    let mut ty = annotation.trim();
    if ((ty.starts_with('\'') && ty.ends_with('\'')) || (ty.starts_with('"') && ty.ends_with('"')))
        && ty.len() >= 2
    {
        ty = &ty[1..ty.len() - 1];
    }
    let compact = python_compact_type(ty);
    let union = split_python_top_level(&compact, '|');
    if union.len() > 1 {
        return canonical_python_union(union.into_iter().map(canonical_python_type));
    }
    if let Some((head, inner)) = python_outer_generic(&compact) {
        let head = canonical_python_type_head(head);
        if head == "Optional" {
            return canonical_python_union([canonical_python_type(inner), "None".to_string()]);
        }
        if head == "Union" {
            return canonical_python_union(
                split_python_top_level(inner, ',')
                    .into_iter()
                    .map(canonical_python_type),
            );
        }
        if head == "Literal" {
            return format!("Literal[{}]", python_compact_type(inner));
        }
        let parts = split_python_top_level(inner, ',');
        let arguments = parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                if head == "Annotated" && index > 0 {
                    python_compact_type(part)
                } else {
                    canonical_python_type(part)
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        return format!("{head}[{arguments}]");
    }
    canonical_python_type_head(&compact)
}

fn python_outer_generic(value: &str) -> Option<(&str, &str)> {
    let open = value.find('[')?;
    let close = matching_python_delimiter(value, open, '[', ']')?;
    (close + 1 == value.len()).then_some((&value[..open], &value[open + 1..close]))
}

fn canonical_python_type_head(head: &str) -> String {
    let bare = head
        .strip_prefix("typing.")
        .or_else(|| head.strip_prefix("typing_extensions."))
        .unwrap_or(head);
    match bare {
        "List" => "list".to_string(),
        "Dict" => "dict".to_string(),
        "Tuple" => "tuple".to_string(),
        "Set" => "set".to_string(),
        "FrozenSet" => "frozenset".to_string(),
        "Type" => "type".to_string(),
        "NoneType" | "types.NoneType" => "None".to_string(),
        _ => bare.to_string(),
    }
}

fn canonical_python_union(parts: impl IntoIterator<Item = String>) -> String {
    let mut parts: Vec<String> = parts.into_iter().collect();
    parts.sort();
    parts.dedup();
    parts.join("|")
}

fn python_type_is_nullable(ty: &str) -> bool {
    if split_python_top_level(ty, '|')
        .iter()
        .any(|part| part.trim() == "None")
    {
        return true;
    }
    let Some((head, inner)) = python_outer_generic(ty) else {
        return false;
    };
    matches!(
        canonical_python_type_head(head).as_str(),
        "Annotated" | "Required" | "NotRequired"
    ) && split_python_top_level(inner, ',')
        .first()
        .is_some_and(|inner| python_type_is_nullable(inner))
}

fn python_outer_type_name(ty: &str) -> Option<&str> {
    python_outer_generic(ty).map(|(head, _)| head)
}

fn python_compact_type(value: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    let mut escape = false;
    for ch in value.chars() {
        if let Some(active) = quote {
            out.push(ch);
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                out.push(ch);
            }
            ch if ch.is_ascii_whitespace() => {}
            _ => out.push(ch),
        }
    }
    out
}

fn split_python_top_level(value: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut round = 0_u32;
    let mut square = 0_u32;
    let mut curly = 0_u32;
    let mut quote = None;
    let mut escape = false;
    for (index, ch) in value.char_indices() {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => round += 1,
            ')' => round = round.saturating_sub(1),
            '[' => square += 1,
            ']' => square = square.saturating_sub(1),
            '{' => curly += 1,
            '}' => curly = curly.saturating_sub(1),
            _ if ch == delimiter && round == 0 && square == 0 && curly == 0 => {
                parts.push(&value[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

fn python_top_level_delimiter(value: &str, delimiter: char) -> Option<usize> {
    let parts = split_python_top_level(value, delimiter);
    (parts.len() > 1).then(|| parts[0].len())
}

fn matching_python_delimiter(
    value: &str,
    open_at: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 0_u32;
    let mut quote = None;
    let mut escape = false;
    for (index, ch) in value
        .char_indices()
        .skip_while(|(index, _)| *index < open_at)
    {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            _ if ch == open => depth += 1,
            _ if ch == close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn collapse_python_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn python_type_alias(line: &str) -> Option<(String, String)> {
    let equal = python_top_level_delimiter(line, '=')?;
    let mut left = line[..equal].trim();
    let target = line[equal + 1..].trim();
    let pep_695 = left.strip_prefix("type ").is_some();
    if pep_695 {
        left = left.strip_prefix("type ")?.trim_start();
    }
    let (name, explicit) = left
        .split_once(':')
        .map_or((left, false), |(name, annotation)| {
            (name.trim(), annotation.contains("TypeAlias"))
        });
    let alias_name = ident_prefix(name)?;
    let remainder = name[alias_name.len()..].trim();
    let valid_type_parameters = remainder.starts_with('[')
        && matching_python_delimiter(remainder, 0, '[', ']') == Some(remainder.len() - 1);
    if alias_name.starts_with('_') || (!remainder.is_empty() && !valid_type_parameters) {
        return None;
    }
    let public_alias = alias_name.chars().next().is_some_and(char::is_uppercase);
    if !pep_695 && !explicit && (!public_alias || !python_alias_target_is_type(target)) {
        return None;
    }
    let mut canonical = canonical_python_type(target);
    if pep_695 && valid_type_parameters {
        canonical = format!("{}=>{canonical}", python_compact_type(remainder));
    }
    Some((alias_name.to_string(), canonical))
}

fn python_alias_target_is_type(target: &str) -> bool {
    let compact = python_compact_type(target);
    if compact.is_empty()
        || compact.starts_with(['\'', '"', '{'])
        || compact.chars().next().is_some_and(|ch| ch.is_ascii_digit())
    {
        return false;
    }
    if let Some(open) = compact.find('(') {
        let bracket = compact.find('[').unwrap_or(usize::MAX);
        if open < bracket {
            return false;
        }
    }
    let head = compact
        .split(['[', '|', ','])
        .next()
        .unwrap_or_default()
        .trim();
    let leaf = head.rsplit('.').next().unwrap_or(head);
    leaf.chars().next().is_some_and(char::is_uppercase)
        || matches!(
            leaf,
            "str"
                | "int"
                | "float"
                | "bool"
                | "bytes"
                | "list"
                | "dict"
                | "tuple"
                | "set"
                | "frozenset"
                | "type"
                | "None"
        )
}

fn parse_python_package_exports(
    rel: &Path,
    package: &str,
    text: &str,
) -> Result<Vec<String>, CoreError> {
    let names = parse_python_all(text)
        .map_err(|message| CoreError::Workspace {
            message: format!(
                "failed to extract Python package exports from {}: {message}",
                rel.display()
            ),
        })?
        .unwrap_or_else(|| parse_python_import_exports(text));
    Ok(names
        .into_iter()
        .filter(|name| !name.starts_with('_'))
        .map(|name| {
            if package == "__init__" {
                name
            } else {
                format!("{package}.{name}")
            }
        })
        .collect())
}

fn parse_python_all(text: &str) -> Result<Option<Vec<String>>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut names = Vec::new();
    let mut declared = false;
    let mut index = 0;
    while index < lines.len() {
        let raw = lines[index];
        let line = strip_python_comment(raw).trim();
        if python_indent(raw) != 0 || !line.starts_with("__all__") {
            index += 1;
            continue;
        }
        let Some(equal) = python_top_level_delimiter(line, '=') else {
            index += 1;
            continue;
        };
        let left = line[..equal].trim_end_matches('+').trim();
        let declaration = left.split_once(':').map_or(left, |(name, _)| name.trim());
        if declaration != "__all__" {
            index += 1;
            continue;
        }
        declared = true;
        let augmented = line[..equal].trim_end().ends_with('+');
        if !augmented {
            names.clear();
        }
        let mut value = line[equal + 1..].trim().to_string();
        while python_has_unclosed_delimiters(&value) && index + 1 < lines.len() {
            index += 1;
            value.push(' ');
            value.push_str(strip_python_comment(lines[index]).trim());
        }
        let parsed = parse_python_all_value(&value)
            .ok_or_else(|| format!("__all__ must be a literal list/tuple (found '{value}')"))?;
        names.extend(parsed);
        index += 1;
    }
    names.sort();
    names.dedup();
    Ok(declared.then_some(names))
}

fn parse_python_all_value(value: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for part in split_python_top_level(value, '+') {
        let part = part.trim();
        let open = part.chars().next()?;
        let close_char = match open {
            '[' => ']',
            '(' => ')',
            _ => return None,
        };
        let close = matching_python_delimiter(part, 0, open, close_char)?;
        if close + close_char.len_utf8() != part.len() {
            return None;
        }
        for item in split_python_top_level(&part[open.len_utf8()..close], ',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            names.push(python_exact_string_literal(item)?);
        }
    }
    Some(names)
}

fn python_exact_string_literal(value: &str) -> Option<String> {
    let quote = value.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let after = &value[quote.len_utf8()..];
    let mut escaped = false;
    for (index, ch) in after.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return after[index + quote.len_utf8()..]
                .trim()
                .is_empty()
                .then(|| after[..index].to_string());
        }
    }
    None
}

fn parse_python_import_exports(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut exports = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if line.starts_with("from ") && line.contains(" import ") {
            let mut declaration = line.to_string();
            while declaration.contains('(') && !declaration.contains(')') && index + 1 < lines.len()
            {
                index += 1;
                declaration.push(' ');
                declaration.push_str(lines[index].trim());
            }
            if let Some((_, names)) = declaration.split_once(" import ") {
                for item in names.trim_matches(|ch| ch == '(' || ch == ')').split(',') {
                    let item = item.trim();
                    if item.is_empty() || item == "*" {
                        continue;
                    }
                    let name = item
                        .split_once(" as ")
                        .map_or(item, |(_, alias)| alias.trim());
                    if ident_prefix(name) == Some(name) {
                        exports.push(name.to_string());
                    }
                }
            }
        } else if let Some(imports) = line.strip_prefix("import ") {
            for item in imports.split(',') {
                let item = item.trim();
                let name = item.split_once(" as ").map_or_else(
                    || item.split('.').next().unwrap_or(item),
                    |(_, alias)| alias.trim(),
                );
                if ident_prefix(name) == Some(name) {
                    exports.push(name.to_string());
                }
            }
        }
        index += 1;
    }
    exports.sort();
    exports.dedup();
    exports
}

#[derive(Default)]
struct PythonImportLayout {
    package_dirs: Vec<(PathBuf, String)>,
}

impl PythonImportLayout {
    fn module_name(&self, path: &Path) -> Option<String> {
        let (relative, package) = if self.package_dirs.is_empty() {
            (path, "")
        } else {
            self.package_dirs.iter().find_map(|(directory, package)| {
                path.strip_prefix(directory)
                    .ok()
                    .map(|relative| (relative, package.as_str()))
            })?
        };
        Some(python_module_name(relative, package))
    }
}

fn python_import_layout(dir: &Path) -> Result<PythonImportLayout, CoreError> {
    let path = dir.join("pyproject.toml");
    if !path.is_file() {
        return Ok(PythonImportLayout::default());
    }
    let text = read_to_string(&path)?;
    let document: toml::Value = toml::from_str(&text).map_err(|error| CoreError::Workspace {
        message: format!(
            "failed to parse Python SDK package metadata {}: {error}",
            path.display()
        ),
    })?;
    let Some(package_dirs) = document
        .get("tool")
        .and_then(|tool| tool.get("setuptools"))
        .and_then(|setuptools| {
            setuptools
                .get("package-dir")
                .or_else(|| setuptools.get("package_dir"))
        })
    else {
        return Ok(PythonImportLayout::default());
    };
    let package_dirs = package_dirs
        .as_table()
        .ok_or_else(|| CoreError::Workspace {
            message: "Python SDK package metadata 'tool.setuptools.package-dir' must be a table"
                .to_string(),
        })?;
    let mut mappings = Vec::new();
    for (package, directory) in package_dirs {
        let directory = directory.as_str().ok_or_else(|| CoreError::Workspace {
            message: format!("Python SDK package directory for '{package}' must be a string"),
        })?;
        mappings.push((python_relative_package_dir(directory)?, package.clone()));
    }
    mappings.sort_by(|(left_path, left_package), (right_path, right_package)| {
        right_path
            .components()
            .count()
            .cmp(&left_path.components().count())
            .then_with(|| left_path.cmp(right_path))
            .then_with(|| left_package.cmp(right_package))
    });
    Ok(PythonImportLayout {
        package_dirs: mappings,
    })
}

fn python_relative_package_dir(value: &str) -> Result<PathBuf, CoreError> {
    let mut path = PathBuf::new();
    for component in Path::new(value).components() {
        match component {
            std::path::Component::Normal(part) => path.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err(CoreError::Workspace {
                    message: format!(
                        "Python SDK package directory must stay within the SDK root: '{value}'"
                    ),
                });
            }
        }
    }
    Ok(path)
}

fn python_module_name(path: &Path, package: &str) -> String {
    let mut parts = python_path_components(path);
    if let Some(last) = parts.last_mut() {
        *last = last.trim_end_matches(".py").to_string();
    }
    if parts.last().is_some_and(|part| part == "__init__") {
        parts.pop();
    }
    let suffix = parts.join(".");
    if package.is_empty() && suffix.is_empty() {
        "__init__".to_string()
    } else if package.is_empty() {
        suffix
    } else if suffix.is_empty() {
        package.to_string()
    } else {
        format!("{package}.{suffix}")
    }
}

fn python_path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str().map(ToString::to_string),
            _ => None,
        })
        .collect()
}

fn is_python_model_file(path: &Path) -> bool {
    path.file_stem().and_then(|stem| stem.to_str()) == Some("models")
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::Normal(value) if value == "models"
            )
        })
}

fn collect_python_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CoreError> {
    let entries = std::fs::read_dir(dir).map_err(|error| CoreError::Workspace {
        message: format!("failed to read Python SDK dir {}: {error}", dir.display()),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| CoreError::Workspace {
            message: format!(
                "failed to read Python SDK dir entry {}: {error}",
                dir.display()
            ),
        })?;
        let path = entry.path();
        let file_type = sdk_entry_file_type(&entry, "Python SDK")?;
        if file_type.is_dir() {
            let skipped = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    matches!(
                        name,
                        ".git" | ".venv" | "venv" | "__pycache__" | "build" | "dist"
                    ) || name.ends_with(".egg-info")
                });
            if !skipped {
                collect_python_files(root, &path, out)?;
            }
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("py")
        {
            let rel = path
                .strip_prefix(root)
                .map_err(|error| CoreError::Workspace {
                    message: format!(
                        "failed to relativize Python file {}: {error}",
                        path.display()
                    ),
                })?
                .to_path_buf();
            out.push(rel);
        }
    }
    Ok(())
}

fn python_package_entry_points(dir: &Path) -> Result<BTreeMap<String, String>, CoreError> {
    let mut entries = BTreeMap::new();
    let pyproject = dir.join("pyproject.toml");
    if pyproject.is_file() {
        let text = read_to_string(&pyproject)?;
        let document: toml::Value =
            toml::from_str(&text).map_err(|error| CoreError::Workspace {
                message: format!(
                    "failed to parse Python SDK package metadata {}: {error}",
                    pyproject.display()
                ),
            })?;
        if let Some(project_value) = document.get("project") {
            let project = python_entry_point_toml_table(project_value, "project")?;
            for table_name in ["scripts", "gui-scripts"] {
                if let Some(value) = project.get(table_name) {
                    let prefix = format!("project.{table_name}");
                    let table = python_entry_point_toml_table(value, &prefix)?;
                    collect_python_entry_point_table(&mut entries, &prefix, table)?;
                }
            }
            if let Some(value) = project.get("entry-points") {
                let groups = python_entry_point_toml_table(value, "project.entry-points")?;
                for (group, value) in groups {
                    let prefix = format!("project.entry-points.{group}");
                    let table = python_entry_point_toml_table(value, &prefix)?;
                    collect_python_entry_point_table(&mut entries, &prefix, table)?;
                }
            }
        }
    }
    let setup_cfg = dir.join("setup.cfg");
    if setup_cfg.is_file() {
        let text = read_to_string(setup_cfg)?;
        let mut in_entry_points = false;
        let mut group = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.starts_with('[') && line.ends_with(']') {
                in_entry_points = line.eq_ignore_ascii_case("[options.entry_points]");
                group.clear();
                continue;
            }
            if !in_entry_points || line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !raw.chars().next().is_some_and(char::is_whitespace) {
                if let Some((name, value)) = line.split_once('=') {
                    group = name.trim().to_string();
                    let value = value.trim();
                    if let Some((name, value)) = value.split_once('=') {
                        entries.insert(
                            format!("setup.cfg.{group}.{}", name.trim()),
                            value.trim().to_string(),
                        );
                    } else if !value.is_empty() {
                        entries.insert(format!("setup.cfg.{group}"), value.to_string());
                    }
                }
            } else if !group.is_empty() {
                if let Some((name, value)) = line.split_once('=') {
                    entries.insert(
                        format!("setup.cfg.{group}.{}", name.trim()),
                        value.trim().to_string(),
                    );
                }
            }
        }
    }
    Ok(entries)
}

fn collect_python_entry_point_table(
    entries: &mut BTreeMap<String, String>,
    prefix: &str,
    table: &toml::map::Map<String, toml::Value>,
) -> Result<(), CoreError> {
    for (name, value) in table {
        let value = value.as_str().ok_or_else(|| CoreError::Workspace {
            message: format!("Python SDK package entry point '{prefix}.{name}' must be a string"),
        })?;
        entries.insert(format!("{prefix}.{name}"), value.to_string());
    }
    Ok(())
}

fn python_entry_point_toml_table<'a>(
    value: &'a toml::Value,
    field: &str,
) -> Result<&'a toml::map::Map<String, toml::Value>, CoreError> {
    value.as_table().ok_or_else(|| CoreError::Workspace {
        message: format!("Python SDK package metadata '{field}' must be a table"),
    })
}

#[derive(Default)]
struct ParsedGoFile {
    type_declarations: BTreeMap<String, String>,
    functions: BTreeMap<String, String>,
    methods: BTreeMap<String, String>,
}

fn parse_go_file(text: &str) -> ParsedGoFile {
    let lines = go_source_lines_without_comments(text);
    let mut parsed = ParsedGoFile {
        type_declarations: extract_go_type_declarations(&lines),
        ..ParsedGoFile::default()
    };
    let mut brace_depth = 0_i32;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if line.is_empty() || line.starts_with("//") {
            index += 1;
            continue;
        }
        if brace_depth == 0 && line.starts_with("func ") {
            let (declaration, last) = collect_go_function_declaration(&lines, index);
            if let Some((name, signature)) = go_func_decl(&declaration) {
                if is_go_exported(&name) {
                    parsed.functions.insert(name, signature);
                }
            }
            if let Some((receiver, method, signature)) = go_method_decl(&declaration) {
                if is_go_exported(&receiver) && is_go_exported(&method) {
                    parsed
                        .methods
                        .insert(format!("{receiver}.{method}"), signature);
                }
            }
            brace_depth += lines[index..=last]
                .iter()
                .map(|line| go_brace_delta(line))
                .sum::<i32>();
            index = last + 1;
            continue;
        }
        brace_depth += go_brace_delta(line);
        index += 1;
    }
    parsed
}

fn extract_go_type_declarations(lines: &[String]) -> BTreeMap<String, String> {
    let mut declarations = BTreeMap::new();
    let mut brace_depth = 0_i32;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if brace_depth == 0 && is_go_type_group_start(line) {
            index += 1;
            while index < lines.len() {
                let spec = lines[index].trim();
                if spec == ")" {
                    break;
                }
                if spec.is_empty() {
                    index += 1;
                    continue;
                }
                let (declaration, last) = collect_go_type_declaration(lines, index, false);
                add_go_type_declaration(&mut declarations, &declaration);
                index = last + 1;
            }
        } else if brace_depth == 0 && go_type_decl(line).is_some() {
            let (declaration, last) = collect_go_type_declaration(lines, index, true);
            add_go_type_declaration(&mut declarations, &declaration);
            brace_depth += lines[index..=last]
                .iter()
                .map(|line| go_brace_delta(line))
                .sum::<i32>();
            index = last + 1;
            continue;
        }
        brace_depth += go_brace_delta(line);
        index += 1;
    }
    declarations
}

fn is_go_type_group_start(line: &str) -> bool {
    line.strip_prefix("type")
        .is_some_and(|rest| rest.trim_start().starts_with('('))
}

fn collect_go_type_declaration(
    lines: &[String],
    start: usize,
    includes_type_keyword: bool,
) -> (String, usize) {
    let mut declaration = if includes_type_keyword {
        String::new()
    } else {
        "type ".to_string()
    };
    let mut index = start;
    while index < lines.len() {
        if !declaration.ends_with(' ') && !declaration.is_empty() {
            declaration.push('\n');
        }
        declaration.push_str(lines[index].trim());
        if go_type_declaration_complete(&declaration) {
            break;
        }
        index += 1;
    }
    (declaration, index.min(lines.len().saturating_sub(1)))
}

fn go_type_declaration_complete(declaration: &str) -> bool {
    if !go_declaration_delimiters_balanced(declaration) {
        return false;
    }
    let trimmed = declaration.trim_end();
    if trimmed.ends_with(" struct") || trimmed.ends_with(" interface") {
        return false;
    }
    !matches!(
        trimmed.chars().last(),
        Some('=' | '|' | '~' | ',' | '(' | '[' | '{')
    )
}

fn add_go_type_declaration(into: &mut BTreeMap<String, String>, declaration: &str) {
    let Some(name) = go_type_decl(declaration.trim_start()) else {
        return;
    };
    if is_go_exported(name) {
        into.insert(name.to_string(), normalize_go_type_declaration(declaration));
    }
}

fn collect_go_function_declaration(lines: &[String], start: usize) -> (String, usize) {
    let mut declaration = String::new();
    let mut index = start;
    while index < lines.len() {
        if !declaration.is_empty() {
            declaration.push(' ');
        }
        declaration.push_str(lines[index].trim());
        if go_function_body_open(&declaration).is_some() {
            break;
        }
        if go_declaration_delimiters_balanced(&declaration) {
            break;
        }
        index += 1;
    }
    (declaration, index.min(lines.len().saturating_sub(1)))
}

fn go_source_lines_without_comments(text: &str) -> Vec<String> {
    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    let mut block_comment = false;
    let mut line_comment = false;
    while let Some(ch) = chars.next() {
        if line_comment {
            if ch == '\n' {
                line_comment = false;
                output.push(ch);
            }
            continue;
        }
        if block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block_comment = false;
            } else if ch == '\n' {
                output.push(ch);
            }
            continue;
        }
        if let Some(active) = quote {
            output.push(ch);
            if active != '`' && escaped {
                escaped = false;
            } else if active != '`' && ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            line_comment = true;
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block_comment = true;
            if !output.ends_with(char::is_whitespace) {
                output.push(' ');
            }
        } else {
            if matches!(ch, '\'' | '"' | '`') {
                quote = Some(ch);
            }
            output.push(ch);
        }
    }
    output.lines().map(ToString::to_string).collect()
}

fn go_declaration_delimiters_balanced(value: &str) -> bool {
    let mut round = 0_i32;
    let mut square = 0_i32;
    let mut curly = 0_i32;
    let mut quote = None;
    let mut escaped = false;
    for ch in value.chars() {
        if let Some(active) = quote {
            if active != '`' && escaped {
                escaped = false;
            } else if active != '`' && ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' => round += 1,
            ')' => round -= 1,
            '[' => square += 1,
            ']' => square -= 1,
            '{' => curly += 1,
            '}' => curly -= 1,
            _ => {}
        }
    }
    round == 0 && square == 0 && curly == 0 && quote.is_none()
}

fn go_brace_delta(value: &str) -> i32 {
    let mut quote = None;
    let mut escaped = false;
    value.chars().fold(0, |mut depth, ch| {
        if let Some(active) = quote {
            if active != '`' && escaped {
                escaped = false;
            } else if active != '`' && ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            return depth;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        depth
    })
}

fn go_function_body_open(value: &str) -> Option<usize> {
    let mut round = 0_u32;
    let mut square = 0_u32;
    let mut curly = 0_u32;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if let Some(active) = quote {
            if active != '`' && escaped {
                escaped = false;
            } else if active != '`' && ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' => round += 1,
            ')' => round = round.saturating_sub(1),
            '[' => square += 1,
            ']' => square = square.saturating_sub(1),
            '{' if round == 0 && square == 0 && curly == 0 => {
                let keyword = value[..index]
                    .trim_end()
                    .rsplit(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                    .next()
                    .unwrap_or_default();
                if matches!(keyword, "struct" | "interface") {
                    curly += 1;
                } else {
                    return Some(index);
                }
            }
            '{' => curly += 1,
            '}' => curly = curly.saturating_sub(1),
            _ => {}
        }
    }
    None
}

fn go_type_decl(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("type ")?;
    ident_prefix(rest)
}

fn go_func_decl(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("func ")?;
    if rest.starts_with('(') {
        return None;
    }
    let name = ident_prefix(rest)?;
    let signature = normalize_go_signature(line);
    Some((name.to_string(), signature))
}

fn go_method_decl(line: &str) -> Option<(String, String, String)> {
    let rest = line.strip_prefix("func ")?;
    let rest = rest.strip_prefix('(')?;
    let receiver_end = rest.find(')')?;
    let receiver = rest[..receiver_end].trim();
    let receiver_type = go_receiver_type(receiver)?;
    let after_receiver = rest[receiver_end + 1..].trim_start();
    let method = ident_prefix(after_receiver)?;
    let signature = normalize_go_signature(line);
    Some((receiver_type, method.to_string(), signature))
}

fn go_receiver_type(receiver: &str) -> Option<String> {
    let raw = receiver.split_whitespace().last()?;
    let raw = raw.trim_start_matches('*');
    let raw = raw.trim_start_matches("[]");
    let name = raw.rsplit('.').next().unwrap_or(raw);
    (!name.is_empty()).then(|| name.to_string())
}

fn normalize_go_signature(line: &str) -> String {
    let signature = go_function_body_open(line)
        .map_or(line, |index| &line[..index])
        .trim();
    normalize_go_func_signature(signature)
        .unwrap_or_else(|| collapse_go_signature_whitespace(signature))
}

fn normalize_go_func_signature(signature: &str) -> Option<String> {
    let rest = signature.strip_prefix("func ")?;
    if rest.starts_with('(') {
        let (receiver, after_receiver) = take_go_parenthesized(rest)?;
        let receiver = normalize_go_param_list(receiver);
        let after_receiver = after_receiver.trim_start();
        let method = ident_prefix(after_receiver)?;
        let after_method = after_receiver[method.len()..].trim_start();
        let (params, after_params) = take_go_parenthesized(after_method)?;
        let params = normalize_go_param_list(params);
        let results = normalize_go_results(after_params.trim());
        return Some(format!("func ({receiver}) {method}({params}){results}"));
    }

    let name = ident_prefix(rest)?;
    let after_name = rest[name.len()..].trim_start();
    let (params, after_params) = take_go_parenthesized(after_name)?;
    let params = normalize_go_param_list(params);
    let results = normalize_go_results(after_params.trim());
    Some(format!("func {name}({params}){results}"))
}

fn normalize_go_results(results: &str) -> String {
    if results.is_empty() {
        return String::new();
    }
    if let Some((list, rest)) = take_go_parenthesized(results) {
        let normalized = normalize_go_param_list(list);
        let suffix = collapse_go_signature_whitespace(rest);
        if !normalized.contains(',') && suffix.is_empty() {
            return format!(" {normalized}");
        }
        if suffix.is_empty() {
            format!(" ({normalized})")
        } else {
            format!(" ({normalized}) {suffix}")
        }
    } else {
        format!(" {}", collapse_go_signature_whitespace(results))
    }
}

fn take_go_parenthesized(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    if !value.starts_with('(') {
        return None;
    }
    let end = matching_go_delimiter(value, 0, '(', ')')?;
    Some((&value[1..end], &value[end + 1..]))
}

fn matching_go_delimiter(value: &str, open_at: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (index, ch) in value
        .char_indices()
        .skip_while(|(index, _)| *index < open_at)
    {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn normalize_go_param_list(list: &str) -> String {
    let mut normalized = Vec::new();
    let mut pending_identifiers = Vec::new();
    for declaration in split_go_top_level_commas(list) {
        let declaration = collapse_go_signature_whitespace(declaration);
        if declaration.is_empty() {
            continue;
        }
        if let Some((names, ty)) = split_go_decl_at_top_level_name_space(&declaration) {
            let name_count = names
                .split(',')
                .filter(|name| !name.trim().is_empty())
                .count();
            let ty = normalize_go_function_type_parameter_names(ty);
            normalized.extend(std::iter::repeat_n(
                ty,
                pending_identifiers.len() + name_count,
            ));
            pending_identifiers.clear();
        } else if is_go_parameter_name(&declaration) {
            pending_identifiers.push(declaration);
        } else {
            normalized.append(&mut pending_identifiers);
            normalized.push(normalize_go_function_type_parameter_names(&declaration));
        }
    }
    normalized.append(&mut pending_identifiers);
    normalized.join(", ")
}

fn normalize_go_type_declaration(declaration: &str) -> String {
    let declaration = normalize_go_interface_method_parameter_names(declaration);
    let declaration = normalize_go_function_type_parameter_names(&declaration);
    let mut tokens = Vec::new();
    let mut chars = declaration.chars().peekable();
    let mut saw_newline = false;
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            saw_newline |= ch == '\n';
            continue;
        }
        if saw_newline
            && tokens
                .last()
                .is_some_and(|token: &String| go_token_can_end_statement(token) && token != ";")
            && ch != '}'
        {
            tokens.push(";".to_string());
        }
        saw_newline = false;
        if ch.is_alphanumeric() || ch == '_' {
            let mut token = String::from(ch);
            while chars
                .peek()
                .is_some_and(|next| next.is_alphanumeric() || *next == '_')
            {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
            tokens.push(token);
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            let mut token = String::from(ch);
            let quote = ch;
            let mut escaped = false;
            for next in chars.by_ref() {
                token.push(next);
                if quote != '`' && escaped {
                    escaped = false;
                } else if quote != '`' && next == '\\' {
                    escaped = true;
                } else if next == quote {
                    break;
                }
            }
            tokens.push(token);
            continue;
        }
        let mut token = String::from(ch);
        if ch == '.' && chars.peek() == Some(&'.') {
            if let Some(next) = chars.next() {
                token.push(next);
            }
            if chars.peek() == Some(&'.') {
                if let Some(next) = chars.next() {
                    token.push(next);
                }
            }
        } else if (matches!(ch, '<' | '>' | ':' | '=' | '!' | '&' | '|')
            && chars.peek() == Some(&'='))
            || (ch == '<' && chars.peek() == Some(&'-'))
        {
            if let Some(next) = chars.next() {
                token.push(next);
            }
        }
        if token == "}" && tokens.last().is_some_and(|previous| previous == ";") {
            tokens.pop();
        }
        tokens.push(token);
    }

    let mut normalized = String::new();
    let mut previous: Option<String> = None;
    for token in tokens {
        if !normalized.is_empty()
            && previous
                .as_deref()
                .is_some_and(|previous| go_tokens_need_space(previous, &token))
        {
            normalized.push(' ');
        }
        normalized.push_str(&token);
        previous = Some(token);
    }
    normalized.trim_end_matches(';').to_string()
}

fn normalize_go_interface_method_parameter_names(value: &str) -> String {
    let Some(interface) = value.find("interface") else {
        return value.to_string();
    };
    if interface > 0
        && value[..interface]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return value.to_string();
    }
    let after_keyword = interface + "interface".len();
    let Some(open_offset) = value[after_keyword..].find('{') else {
        return value.to_string();
    };
    let open = after_keyword + open_offset;
    let Some(close) = matching_go_delimiter(value, open, '{', '}') else {
        return value.to_string();
    };
    let body = normalize_go_interface_body(&value[open + 1..close]);
    let mut normalized = String::with_capacity(value.len());
    normalized.push_str(&value[..=open]);
    normalized.push_str(&body);
    normalized.push_str(&value[close..]);
    normalized
}

fn normalize_go_interface_body(body: &str) -> String {
    let mut normalized = String::with_capacity(body.len());
    let mut cursor = 0usize;
    let mut nested_braces = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < body.len() {
        let Some(ch) = body[cursor..].chars().next() else {
            break;
        };
        if let Some(active) = quote {
            normalized.push(ch);
            cursor += ch.len_utf8();
            if active != '`' && escaped {
                escaped = false;
            } else if active != '`' && ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            quote = Some(ch);
            normalized.push(ch);
            cursor += ch.len_utf8();
            continue;
        }
        if ch == '{' {
            nested_braces += 1;
        } else if ch == '}' {
            nested_braces = nested_braces.saturating_sub(1);
        }
        if nested_braces == 0 {
            let remaining = &body[cursor..];
            if let Some(method) = ident_prefix(remaining) {
                let after_method = cursor + method.len();
                let params = skip_go_whitespace(body, after_method);
                if body[params..].starts_with('(') {
                    let Some(params_end) = matching_go_delimiter(body, params, '(', ')') else {
                        normalized.push_str(remaining);
                        break;
                    };
                    normalized.push_str(method);
                    normalized.push_str(&body[after_method..=params]);
                    normalized.push_str(&normalize_go_param_list(&body[params + 1..params_end]));
                    normalized.push(')');
                    cursor = params_end + 1;

                    let results = skip_go_whitespace(body, cursor);
                    if body[results..].starts_with('(') {
                        let Some(results_end) = matching_go_delimiter(body, results, '(', ')')
                        else {
                            normalized.push_str(&body[cursor..]);
                            break;
                        };
                        normalized.push_str(&body[cursor..=results]);
                        normalized
                            .push_str(&normalize_go_param_list(&body[results + 1..results_end]));
                        normalized.push(')');
                        cursor = results_end + 1;
                    }
                    continue;
                }
            }
        }
        normalized.push(ch);
        cursor += ch.len_utf8();
    }
    normalized
}

fn normalize_go_function_type_parameter_names(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut cursor = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < value.len() {
        let Some(ch) = value[cursor..].chars().next() else {
            break;
        };
        if let Some(active) = quote {
            normalized.push(ch);
            cursor += ch.len_utf8();
            if active != '`' && escaped {
                escaped = false;
            } else if active != '`' && ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            quote = Some(ch);
            normalized.push(ch);
            cursor += ch.len_utf8();
            continue;
        }
        if value[cursor..].starts_with("func") && go_keyword_boundary(value, cursor, 4) {
            let after_keyword = cursor + 4;
            let params = skip_go_whitespace(value, after_keyword);
            if value[params..].starts_with('(') {
                let Some(params_end) = matching_go_delimiter(value, params, '(', ')') else {
                    normalized.push_str(&value[cursor..]);
                    break;
                };
                normalized.push_str(&value[cursor..=params]);
                normalized.push_str(&normalize_go_param_list(&value[params + 1..params_end]));
                normalized.push(')');
                cursor = params_end + 1;

                let results = skip_go_whitespace(value, cursor);
                if value[results..].starts_with('(') {
                    let Some(results_end) = matching_go_delimiter(value, results, '(', ')') else {
                        normalized.push_str(&value[cursor..]);
                        break;
                    };
                    normalized.push_str(&value[cursor..=results]);
                    normalized.push_str(&normalize_go_param_list(&value[results + 1..results_end]));
                    normalized.push(')');
                    cursor = results_end + 1;
                }
                continue;
            }
        }
        normalized.push(ch);
        cursor += ch.len_utf8();
    }
    normalized
}

fn skip_go_whitespace(value: &str, mut cursor: usize) -> usize {
    while cursor < value.len() {
        let Some(ch) = value[cursor..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        cursor += ch.len_utf8();
    }
    cursor
}

fn go_keyword_boundary(value: &str, start: usize, len: usize) -> bool {
    let before = value[..start].chars().next_back();
    let after = value[start + len..].chars().next();
    let is_identifier = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    before.is_none_or(|ch| !is_identifier(ch)) && after.is_none_or(|ch| !is_identifier(ch))
}

fn go_token_can_end_statement(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || matches!(ch, '\'' | '"' | '`'))
        || matches!(token, ")" | "]" | "}")
}

fn go_tokens_need_space(previous: &str, current: &str) -> bool {
    let atom = |token: &str| {
        token
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || matches!(ch, '\'' | '"' | '`'))
    };
    atom(previous) && atom(current)
}

fn split_go_decl_at_top_level_name_space(value: &str) -> Option<(&str, &str)> {
    let mut paren = 0usize;
    let mut bracket = 0usize;
    let mut brace = 0usize;
    let mut current_space_start: Option<usize> = None;

    for (index, ch) in value.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            _ => {}
        }
        let at_top = paren == 0 && bracket == 0 && brace == 0;
        if at_top && ch.is_whitespace() {
            current_space_start.get_or_insert(index);
        } else if let Some(start) = current_space_start.take() {
            let names = value[..start].trim();
            let ty = value[index..].trim();
            if !names.is_empty() && !ty.is_empty() && is_go_parameter_name_list(names) {
                return Some((names, ty));
            }
        }
    }
    None
}

fn split_go_top_level_commas(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut paren = 0usize;
    let mut bracket = 0usize;
    let mut brace = 0usize;
    for (index, ch) in value.char_indices() {
        match ch {
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => bracket += 1,
            ']' => bracket = bracket.saturating_sub(1),
            '{' => brace += 1,
            '}' => brace = brace.saturating_sub(1),
            ',' if paren == 0 && bracket == 0 && brace == 0 => {
                out.push(value[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(value[start..].trim());
    out
}

fn is_go_parameter_name_list(value: &str) -> bool {
    value.split(',').map(str::trim).all(is_go_parameter_name)
}

fn is_go_parameter_name(value: &str) -> bool {
    !value.is_empty()
        && ident_prefix(value) == Some(value)
        && !matches!(
            value,
            "break"
                | "case"
                | "chan"
                | "const"
                | "continue"
                | "default"
                | "defer"
                | "else"
                | "fallthrough"
                | "for"
                | "func"
                | "go"
                | "goto"
                | "if"
                | "import"
                | "interface"
                | "map"
                | "package"
                | "range"
                | "return"
                | "select"
                | "struct"
                | "switch"
                | "type"
                | "var"
        )
}

fn collapse_go_signature_whitespace(signature: &str) -> String {
    signature.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_go_exported(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn qualify_go_symbol(path: &Path, symbol: &str) -> String {
    let Some(parent) = path.parent() else {
        return symbol.to_string();
    };
    let package_path = parent
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if package_path.is_empty() {
        symbol.to_string()
    } else {
        format!("{package_path}.{symbol}")
    }
}

fn collect_go_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), CoreError> {
    let entries = std::fs::read_dir(dir).map_err(|err| CoreError::Workspace {
        message: format!("failed to read Go SDK dir {}: {err}", dir.display()),
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| CoreError::Workspace {
            message: format!("failed to read Go SDK dir entry {}: {err}", dir.display()),
        })?;
        let path = entry.path();
        let file_type = sdk_entry_file_type(&entry, "Go SDK")?;
        if file_type.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_ignored_go_directory)
            {
                continue;
            }
            collect_go_files(root, &path, out)?;
        } else if file_type.is_file()
            && path.extension().and_then(|ext| ext.to_str()) == Some("go")
            && !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_test.go"))
        {
            let rel = path
                .strip_prefix(root)
                .map_err(|err| CoreError::Workspace {
                    message: format!("failed to relativize Go file {}: {err}", path.display()),
                })?
                .to_path_buf();
            out.push(rel);
        }
    }
    Ok(())
}

fn is_ignored_go_directory(name: &str) -> bool {
    matches!(name, "vendor" | "internal" | "testdata")
        || name.starts_with('.')
        || name.starts_with('_')
}

fn go_package_metadata(dir: &Path) -> Result<BTreeMap<String, String>, CoreError> {
    let path = dir.join("go.mod");
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = read_to_string(path)?;
    let mut out = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if let Some(module) = line.strip_prefix("module ") {
            out.insert("module".to_string(), module.trim().to_string());
        } else if let Some(version) = line.strip_prefix("go ") {
            out.insert("go".to_string(), version.trim().to_string());
        } else if let Some(requirement) = line.strip_prefix("require ") {
            let requirement = requirement.trim();
            if !requirement.starts_with('(') && !requirement.is_empty() {
                out.insert(format!("require:{requirement}"), requirement.to_string());
            }
        }
    }
    Ok(out)
}

fn doc_files(dir: &Path) -> Result<Vec<String>, CoreError> {
    let mut out = Vec::new();
    collect_doc_files(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_doc_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), CoreError> {
    let entries = std::fs::read_dir(dir).map_err(|err| CoreError::Workspace {
        message: format!("failed to read SDK doc dir {}: {err}", dir.display()),
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| CoreError::Workspace {
            message: format!("failed to read SDK doc dir entry {}: {err}", dir.display()),
        })?;
        let path = entry.path();
        let file_type = sdk_entry_file_type(&entry, "SDK documentation")?;
        if file_type.is_dir() {
            if matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("vendor" | "node_modules")
            ) {
                continue;
            }
            collect_doc_files(root, &path, out)?;
        } else if file_type.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            let rel = path
                .strip_prefix(root)
                .map_err(|err| CoreError::Workspace {
                    message: format!(
                        "failed to relativize SDK doc file {}: {err}",
                        path.display()
                    ),
                })?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}

fn missing_keys(
    old: &BTreeMap<String, TsExportKind>,
    new: &BTreeMap<String, TsExportKind>,
) -> Vec<String> {
    old.keys()
        .filter(|key| !new.contains_key(*key))
        .cloned()
        .collect()
}

fn missing_string_keys(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<String> {
    old.keys()
        .filter(|key| !new.contains_key(*key))
        .cloned()
        .collect()
}

fn kind_mismatches(
    old: &BTreeMap<String, TsExportKind>,
    new: &BTreeMap<String, TsExportKind>,
) -> Vec<TsExportKindMismatch> {
    old.iter()
        .filter_map(|(symbol, old_kind)| {
            let new_kind = new.get(symbol)?;
            (old_kind != new_kind).then(|| TsExportKindMismatch {
                symbol: symbol.clone(),
                old: *old_kind,
                new: *new_kind,
            })
        })
        .collect()
}

fn missing_values(old: &[String], new: &[String]) -> Vec<String> {
    let new: BTreeSet<&String> = new.iter().collect();
    old.iter()
        .filter(|value| !new.contains(*value))
        .cloned()
        .collect()
}

fn package_changes(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<String> {
    old.iter()
        .filter_map(|(key, old_value)| match new.get(key) {
            Some(new_value) if new_value == old_value => None,
            Some(new_value) => Some(format!("{key}: {old_value} -> {new_value}")),
            None => Some(format!("{key}: removed {old_value}")),
        })
        .collect()
}

fn go_signature_changes(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<GoSignatureChange> {
    old.iter()
        .filter_map(|(symbol, old_signature)| {
            let new_signature = new.get(symbol)?;
            (old_signature != new_signature).then(|| GoSignatureChange {
                symbol: symbol.clone(),
                old: old_signature.clone(),
                new: new_signature.clone(),
            })
        })
        .collect()
}

fn go_type_declaration_changes(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<GoTypeDeclarationChange> {
    old.iter()
        .filter_map(|(symbol, old_declaration)| {
            let new_declaration = new.get(symbol)?;
            (old_declaration != new_declaration).then(|| GoTypeDeclarationChange {
                symbol: symbol.clone(),
                old: old_declaration.clone(),
                new: new_declaration.clone(),
            })
        })
        .collect()
}

fn missing_interface_properties(
    old: &BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
    new: &BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
) -> Vec<TsMissingInterfaceProperty> {
    let mut missing = Vec::new();
    for (interface, old_props) in old {
        let new_props = new.get(interface);
        for property in old_props.keys() {
            if new_props.is_none_or(|props| !props.contains_key(property)) {
                missing.push(TsMissingInterfaceProperty {
                    interface: interface.clone(),
                    property: property.clone(),
                });
            }
        }
    }
    missing
}

fn interface_property_changes(
    old: &BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
    new: &BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
) -> Vec<TsInterfacePropertyChange> {
    let mut changes = Vec::new();
    for (interface, old_props) in old {
        let Some(new_props) = new.get(interface) else {
            continue;
        };
        for (property, old_shape) in old_props {
            let Some(new_shape) = new_props.get(property) else {
                continue;
            };
            if old_shape != new_shape {
                changes.push(TsInterfacePropertyChange {
                    interface: interface.clone(),
                    property: property.clone(),
                    old: old_shape.clone(),
                    new: new_shape.clone(),
                });
            }
        }
    }
    changes
}

fn interface_required_to_optional(
    changes: &[TsInterfacePropertyChange],
) -> Vec<TsInterfacePropertyChange> {
    changes
        .iter()
        .filter(|change| !change.old.optional && change.new.optional)
        .cloned()
        .collect()
}

fn interface_optional_to_required(
    changes: &[TsInterfacePropertyChange],
) -> Vec<TsInterfacePropertyChange> {
    changes
        .iter()
        .filter(|change| change.old.optional && !change.new.optional)
        .cloned()
        .collect()
}

fn interface_nullable_changes(
    changes: &[TsInterfacePropertyChange],
) -> Vec<TsInterfacePropertyChange> {
    changes
        .iter()
        .filter(|change| change.old.nullable != change.new.nullable)
        .cloned()
        .collect()
}

fn interface_type_changes(changes: &[TsInterfacePropertyChange]) -> Vec<TsInterfacePropertyChange> {
    changes
        .iter()
        .filter(|change| non_null_ts_type(&change.old.ty) != non_null_ts_type(&change.new.ty))
        .cloned()
        .collect()
}

fn ts_type_declaration_changes(
    old: &BTreeMap<String, Vec<String>>,
    new: &BTreeMap<String, Vec<String>>,
) -> Vec<TsTypeDeclarationChange> {
    old.iter()
        .filter_map(|(symbol, old_shapes)| match new.get(symbol) {
            Some(new_shapes) if old_shapes != new_shapes => Some(TsTypeDeclarationChange {
                symbol: symbol.clone(),
                old: old_shapes.clone(),
                new: new_shapes.clone(),
            }),
            None => Some(TsTypeDeclarationChange {
                symbol: symbol.clone(),
                old: old_shapes.clone(),
                new: Vec::new(),
            }),
            Some(_) => None,
        })
        .collect()
}

fn non_null_ts_type(ty: &str) -> String {
    ty.split('|')
        .map(str::trim)
        .filter(|part| *part != "null")
        .collect::<Vec<_>>()
        .join(" | ")
}

fn operation_return_type_changes(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<TsOperationReturnTypeChange> {
    old.iter()
        .filter_map(|(operation, old_ty)| match new.get(operation) {
            Some(new_ty) if old_ty != new_ty => Some(TsOperationReturnTypeChange {
                operation: operation.clone(),
                old: old_ty.clone(),
                new: new_ty.clone(),
            }),
            None => Some(TsOperationReturnTypeChange {
                operation: operation.clone(),
                old: old_ty.clone(),
                new: "<missing>".to_string(),
            }),
            Some(_) => None,
        })
        .collect()
}

fn operation_signature_changes(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
) -> Vec<TsOperationSignatureChange> {
    old.iter()
        .filter_map(|(operation, old_signature)| match new.get(operation) {
            Some(new_signature) if old_signature != new_signature => {
                Some(TsOperationSignatureChange {
                    operation: operation.clone(),
                    old: old_signature.clone(),
                    new: new_signature.clone(),
                })
            }
            None => Some(TsOperationSignatureChange {
                operation: operation.clone(),
                old: old_signature.clone(),
                new: "<missing>".to_string(),
            }),
            Some(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{
        diff_go_dirs, diff_go_surfaces, diff_python_dirs, diff_typescript_dirs,
        diff_typescript_surfaces, evaluate_go_contract, evaluate_typescript_contract,
        extract_go_surface, extract_python_surface, extract_typescript_surface, suggest_go_compat,
        suggest_typescript_compat, GoCompatibilityContract, GoSignatureChange, GoSurface,
        GoSurfaceDiff, TsExportKind, TsExportKindMismatch, TsInterfaceProperty,
        TypeScriptCompatibilityContract, TypeScriptSurface,
    };
    use std::collections::BTreeMap;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "gnr8-ts-compat-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extracts_typescript_surface() {
        let dir = temp_dir("surface");
        std::fs::write(
            dir.join("index.ts"),
            "export * from \"./models\";\nexport * from \"./api\";\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("models.ts"),
            "export interface Book {\n  title?: string | null;\n}\nexport const Format = {} as const;\nexport type Format = typeof Format[keyof typeof Format];\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("api.ts"),
            "export interface CreateBookRequest {}\nexport class DefaultApi {\n  async createBook(): Promise<AxiosResponse<Book>> {\n    if (true) {\n      return response;\n    }\n  }\n  async listBooks(): Promise<void> { return; }\n}\nexport const DefaultApiFactory = function () {\n  return {\n    createBook(): Promise<AxiosResponse<Book>> { return api.createBook(); },\n  };\n};\n",
        )
        .unwrap();

        let surface = extract_typescript_surface(&dir).unwrap();
        assert_eq!(surface.root_exports["Book"], TsExportKind::Type);
        assert_eq!(surface.root_exports["Format"], TsExportKind::Both);
        assert!(surface.interface_properties["Book"]["title"].optional);
        assert!(surface.interface_properties["Book"]["title"].nullable);
        assert_eq!(surface.api_classes, vec!["DefaultApi"]);
        assert_eq!(surface.api_factories, vec!["DefaultApiFactory"]);
        assert_eq!(
            surface.operation_methods,
            vec!["DefaultApi.createBook", "DefaultApi.listBooks"]
        );
        assert_eq!(
            surface.operation_return_types["DefaultApi.createBook"],
            "Promise<AxiosResponse<Book>>"
        );
        assert_eq!(
            surface.operation_signatures["DefaultApi.createBook"],
            "async createBook(): Promise<AxiosResponse<Book>>"
        );
        assert_eq!(
            surface.operation_return_types["DefaultApiFactory.createBook"],
            "Promise<AxiosResponse<Book>>"
        );
        assert_eq!(
            surface.operation_signatures["DefaultApiFactory.createBook"],
            "createBook(): Promise<AxiosResponse<Book>>"
        );
        assert_eq!(surface.request_aliases, vec!["CreateBookRequest"]);
        assert!(surface.docs.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn root_exports_follow_reexport_targets() {
        let dir = temp_dir("root-targets");
        std::fs::write(dir.join("index.ts"), "export * from \"./models\";\n").unwrap();
        std::fs::write(dir.join("models.ts"), "export interface Book {}\n").unwrap();
        std::fs::write(dir.join("api.ts"), "export class DefaultApi {}\n").unwrap();

        let surface = extract_typescript_surface(&dir).unwrap();
        assert!(surface.root_exports.contains_key("Book"));
        assert!(
            !surface.root_exports.contains_key("DefaultApi"),
            "api.ts was not re-exported from root: {:?}",
            surface.root_exports
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_typescript_barrel_does_not_export_private_modules() {
        let old = temp_dir("ts-root-barrel-old");
        let new = temp_dir("ts-root-barrel-new");
        std::fs::write(old.join("index.ts"), "export * from \"./models\";\n").unwrap();
        std::fs::write(
            new.join("index.ts"),
            "// Intentionally no public exports.\n",
        )
        .unwrap();
        for dir in [&old, &new] {
            std::fs::write(dir.join("models.ts"), "export interface Book {}\n").unwrap();
        }

        let new_surface = extract_typescript_surface(&new).unwrap();
        assert!(new_surface.root_exports.is_empty());
        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert_eq!(diff.missing_root_exports, vec!["Book"]);

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_semantic_surface_ignores_unreachable_private_modules() {
        let old = temp_dir("ts-private-semantics-old");
        let new = temp_dir("ts-private-semantics-new");
        for dir in [&old, &new] {
            std::fs::write(
                dir.join("index.ts"),
                "export { type InternalBook as Book } from \"./public\";\n",
            )
            .unwrap();
            std::fs::write(
                dir.join("public.ts"),
                "export interface InternalBook { title: string; }\n",
            )
            .unwrap();
        }
        std::fs::write(
            old.join("private.ts"),
            "export interface Hidden { value: string; }\nexport class HiddenApi { async read(): Promise<string> {} }\n",
        )
        .unwrap();
        std::fs::write(
            new.join("private.ts"),
            "export interface Hidden { value: number; }\nexport class HiddenApi { async read(): Promise<number> {} }\n",
        )
        .unwrap();

        let surface = extract_typescript_surface(&old).unwrap();
        assert!(surface.interface_properties.contains_key("Book"));
        assert!(!surface.interface_properties.contains_key("InternalBook"));
        assert!(!surface.interface_properties.contains_key("Hidden"));
        assert!(!surface.api_classes.contains(&"HiddenApi".to_string()));
        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert!(!diff.is_breaking(), "private module drift leaked: {diff:?}");

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_semantic_surface_follows_stable_public_aliases() {
        let old = temp_dir("ts-public-alias-old");
        let new = temp_dir("ts-public-alias-new");
        std::fs::write(
            old.join("index.ts"),
            "export { type OldInternalBook as Book } from \"./old-layout\";\n",
        )
        .unwrap();
        std::fs::write(
            old.join("old-layout.ts"),
            "export interface OldInternalBook {\n  title: string;\n  next?: OldInternalBook;\n}\n",
        )
        .unwrap();
        std::fs::write(
            new.join("index.ts"),
            "export { type NewInternalBook as Book } from \"./new-layout\";\n",
        )
        .unwrap();
        std::fs::write(
            new.join("new-layout.ts"),
            "export interface NewInternalBook {\n  title: string;\n  next?: NewInternalBook;\n}\n",
        )
        .unwrap();

        let surface = extract_typescript_surface(&old).unwrap();
        assert!(surface.interface_properties.contains_key("Book"));
        assert!(!surface.interface_properties.contains_key("OldInternalBook"));
        assert!(surface.type_declarations["Book"]
            .iter()
            .all(|shape| !shape.contains("OldInternalBook")));
        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert!(
            !diff.is_breaking(),
            "private declaration identity leaked into the public alias: {diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_public_aliases_still_report_semantic_drift() {
        let old = temp_dir("ts-public-alias-drift-old");
        let new = temp_dir("ts-public-alias-drift-new");
        for dir in [&old, &new] {
            std::fs::write(
                dir.join("index.ts"),
                "export { type InternalBook as Book } from \"./internal\";\n",
            )
            .unwrap();
        }
        std::fs::write(
            old.join("internal.ts"),
            "export interface InternalBook {\n  title: string;\n}\n",
        )
        .unwrap();
        std::fs::write(
            new.join("internal.ts"),
            "export interface InternalBook {\n  title: number;\n}\n",
        )
        .unwrap();

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert_eq!(diff.interface_type_changes.len(), 1);
        assert_eq!(diff.interface_type_changes[0].interface, "Book");

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_alias_canonicalization_preserves_public_property_names() {
        let old = temp_dir("ts-alias-property-old");
        let stable = temp_dir("ts-alias-property-stable");
        let changed = temp_dir("ts-alias-property-changed");
        std::fs::write(
            old.join("index.ts"),
            "export { type OldInternal as Book } from \"./internal\";\n",
        )
        .unwrap();
        std::fs::write(
            old.join("internal.ts"),
            "export type OldInternal = { OldInternal: string; next?: OldInternal };\n",
        )
        .unwrap();
        for dir in [&stable, &changed] {
            std::fs::write(
                dir.join("index.ts"),
                "export { type NewInternal as Book } from \"./internal\";\n",
            )
            .unwrap();
        }
        std::fs::write(
            stable.join("internal.ts"),
            "export type NewInternal = { OldInternal: string; next?: NewInternal };\n",
        )
        .unwrap();
        std::fs::write(
            changed.join("internal.ts"),
            "export type NewInternal = { NewInternal: string; next?: NewInternal };\n",
        )
        .unwrap();

        let stable_diff = diff_typescript_dirs(&old, &stable).unwrap();
        assert!(
            !stable_diff.is_breaking(),
            "a property key was mistaken for the private declaration: {stable_diff:?}"
        );
        let changed_diff = diff_typescript_dirs(&old, &changed).unwrap();
        assert_eq!(changed_diff.type_declaration_changes.len(), 1);
        assert_eq!(changed_diff.type_declaration_changes[0].symbol, "Book");

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(stable);
        let _ = std::fs::remove_dir_all(changed);
    }

    #[test]
    fn typescript_declaration_file_barrels_define_the_public_root() {
        let dir = temp_dir("ts-declaration-barrel");
        std::fs::write(
            dir.join("index.d.ts"),
            "export type { Book } from \"./models\";\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("models.d.ts"),
            "export interface Book { title: string; }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("private.d.ts"),
            "export interface Hidden { value: string; }\n",
        )
        .unwrap();

        let surface = extract_typescript_surface(&dir).unwrap();
        assert_eq!(
            surface.root_exports,
            BTreeMap::from([("Book".to_string(), TsExportKind::Type)])
        );
        assert!(surface.interface_properties.contains_key("Book"));
        assert!(!surface.interface_properties.contains_key("Hidden"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typescript_extracts_multiline_modified_methods_without_structural_noise() {
        let dir = temp_dir("ts-multiline-methods");
        std::fs::write(
            dir.join("index.ts"),
            r"export default abstract class BooksApi {
  // A stray closing brace in a comment must not end the class: }
  public async getBook(
    id: string,
    options?: { trace?: boolean },
  ): Promise<
    Book
  > {
    const diagnostic = `body braces are not declarations: } {`;
    const closingBrace = /}/;
    return this.fetch(id, options);
  }

  listBooks(
    options?: RequestOptions,
  ): Promise<Book[]> {
    return this.fetchAll(options);
  }

  protected async internalOnly(): Promise<void> {}
  private hidden(): void {}
}
",
        )
        .unwrap();

        let surface = extract_typescript_surface(&dir).unwrap();
        assert_eq!(surface.root_exports["default"], TsExportKind::Both);
        assert_eq!(surface.api_classes, vec!["default"]);
        assert_eq!(
            surface.operation_methods,
            vec!["default.getBook", "default.listBooks"]
        );
        assert_eq!(
            surface.operation_return_types["default.getBook"],
            "Promise<Book>"
        );
        assert_eq!(
            surface.operation_signatures["default.getBook"],
            "async getBook(id: string, options?: { trace?: boolean }): Promise<Book>"
        );
        assert_eq!(
            surface.operation_signatures["default.listBooks"],
            "listBooks(options?: RequestOptions): Promise<Book[]>"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typescript_default_export_internal_rename_is_not_public_drift() {
        let old = temp_dir("ts-default-class-old");
        let new = temp_dir("ts-default-class-new");
        std::fs::write(
            old.join("index.ts"),
            "export default class InternalOld { getBook(): Promise<string> { throw new Error(); } }\n",
        )
        .unwrap();
        std::fs::write(
            new.join("index.ts"),
            "export default class InternalNew { getBook(): Promise<string> { throw new Error(); } }\n",
        )
        .unwrap();

        let diff = diff_typescript_dirs(&old, &new).unwrap();

        assert!(
            !diff.is_breaking(),
            "the local name behind a default export is not public identity: {diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_named_reexports_preserve_aliases_and_namespace_kinds() {
        let dir = temp_dir("ts-named-reexports");
        std::fs::write(
            dir.join("index.ts"),
            r#"export {
  type Book as PublishedBook,
  BookState as State,
  BooksApi,
} from "./public";
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("public.ts"),
            r#"export interface Book {}
export const BookState = { available: "available" } as const;
export class BooksApi {}
export default class InternalDefault {}
"#,
        )
        .unwrap();

        let surface = extract_typescript_surface(&dir).unwrap();
        assert_eq!(
            surface.root_exports,
            BTreeMap::from([
                ("BooksApi".to_string(), TsExportKind::Both),
                ("PublishedBook".to_string(), TsExportKind::Type),
                ("State".to_string(), TsExportKind::Value),
            ])
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typescript_local_exports_retain_declaration_kinds_and_shapes() {
        let old = temp_dir("ts-local-exports-old");
        let new = temp_dir("ts-local-exports-new");
        std::fs::write(
            old.join("index.ts"),
            r"interface Book {
  title: string;
}
class BooksApi {
  getBook(): Promise<Book> { throw new Error(); }
}
function privateScope() {
  interface Hidden { value: string; }
}
export { Book, BooksApi };
",
        )
        .unwrap();
        std::fs::write(
            new.join("index.ts"),
            r"interface Book {
  title: number;
}
class BooksApi {
  getBook(): Promise<Book> { throw new Error(); }
}
function privateScope() {
  interface Hidden { value: number; }
}
export { Book, BooksApi };
",
        )
        .unwrap();

        let surface = extract_typescript_surface(&old).unwrap();
        assert_eq!(surface.root_exports["Book"], TsExportKind::Type);
        assert_eq!(surface.root_exports["BooksApi"], TsExportKind::Both);
        assert_eq!(surface.api_classes, vec!["BooksApi"]);
        assert_eq!(surface.operation_methods, vec!["BooksApi.getBook"]);
        assert!(!surface.interface_properties.contains_key("Hidden"));

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert_eq!(diff.interface_type_changes.len(), 1, "{diff:?}");
        assert_eq!(diff.interface_type_changes[0].interface, "Book");
        assert!(
            diff.type_declaration_changes
                .iter()
                .all(|change| change.symbol != "Hidden"),
            "nested private declarations are not package exports: {diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_export_star_does_not_reexport_default() {
        let dir = temp_dir("ts-star-default");
        std::fs::write(dir.join("index.ts"), "export * from \"./public\";\n").unwrap();
        std::fs::write(
            dir.join("public.ts"),
            "export default class HiddenDefault {}\nexport interface Book {}\n",
        )
        .unwrap();

        let surface = extract_typescript_surface(&dir).unwrap();
        assert_eq!(
            surface.root_exports,
            BTreeMap::from([("Book".to_string(), TsExportKind::Type)])
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typescript_diff_catches_alias_heritage_and_enum_like_value_changes() {
        let old = temp_dir("ts-declarations-old");
        let new = temp_dir("ts-declarations-new");
        std::fs::write(
            old.join("index.ts"),
            r#"export type BookFormat =
  "paperback"
  | "hardcover";
export interface Book<T extends string = string> extends Audited<T> {
  title: string;
}
export const Availability = {
  Available: "available",
  Gone: "gone",
} as const;
export type Availability = typeof Availability[keyof typeof Availability];
"#,
        )
        .unwrap();
        std::fs::write(
            new.join("index.ts"),
            r#"export type BookFormat = "paperback" | number;
export interface Book<T extends string = string> extends Entity<T> {
  title: string;
}
export const Availability = {
  Available: "available",
} as const;
export type Availability = typeof Availability[keyof typeof Availability];
"#,
        )
        .unwrap();

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert_eq!(
            diff.type_declaration_changes
                .iter()
                .map(|change| change.symbol.as_str())
                .collect::<Vec<_>>(),
            vec!["Availability", "Book", "BookFormat"]
        );
        assert!(diff.is_breaking());

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_declaration_shapes_ignore_formatting_only_changes() {
        let old = temp_dir("ts-declarations-format-old");
        let new = temp_dir("ts-declarations-format-new");
        std::fs::write(
            old.join("index.ts"),
            "export type BookFormat =\n  \"paperback\"\n  | \"hardcover\";\nexport interface Book\n  extends Audited<BookFormat>\n{\n  title: string;\n}\n",
        )
        .unwrap();
        std::fs::write(
            new.join("index.ts"),
            "export type BookFormat = \"paperback\" | \"hardcover\";\nexport interface Book extends Audited<BookFormat> { title: string; }\n",
        )
        .unwrap();

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert!(
            diff.type_declaration_changes.is_empty(),
            "formatting-only declarations should compare equal: {:?}",
            diff.type_declaration_changes
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn extracts_and_diffs_python_package_surface() {
        let old = temp_dir("python-old");
        let new = temp_dir("python-new");
        for dir in [&old, &new] {
            std::fs::create_dir_all(dir.join("sdk")).unwrap();
        }
        std::fs::write(
            old.join("sdk/__init__.py"),
            "from .errors import ApiError\nfrom .models import Book, BookAlias\n\n__all__ = [\n    \"ApiError\",\n    \"Book\",\n    \"BookAlias\",\n]\n",
        )
        .unwrap();
        std::fs::write(
            new.join("sdk/__init__.py"),
            "from .errors import ClientError\nfrom .models import Book\n\n__all__ = [\"ClientError\", \"Book\"]\n",
        )
        .unwrap();
        std::fs::write(
            old.join("sdk/models.py"),
            "from typing import Optional, TypeAlias\n\nclass Book(BaseModel):\n    title: str = Field(alias=\"bookTitle\")\n    note: Optional[str] = None\n\n    def __init__(self, title: str, note: Optional[str] = None):\n        pass\n\n    @classmethod\n    def from_dict(cls, data):\n        return cls(**data)\n\n    def to_dict(self):\n        return {}\n\nBookAlias: TypeAlias = Book\n",
        )
        .unwrap();
        std::fs::write(
            new.join("sdk/models.py"),
            "class Book(BaseModel):\n    title: str = Field(default=None, alias=\"title\")\n    note: str | None = None\n\n    def __init__(self, title: str | None = None, note: str | None = None):\n        pass\n\n    def to_dict(self):\n        return {}\n",
        )
        .unwrap();
        std::fs::write(
            old.join("sdk/errors.py"),
            "class ApiError(Exception):\n    pass\n",
        )
        .unwrap();
        std::fs::write(
            new.join("sdk/errors.py"),
            "class ClientError(Exception):\n    pass\n",
        )
        .unwrap();
        std::fs::write(old.join("sdk/legacy.py"), "VALUE = 1\n").unwrap();
        std::fs::write(
            old.join("pyproject.toml"),
            "[project.scripts]\nsdk = \"sdk.cli:main\"\n",
        )
        .unwrap();
        std::fs::write(
            new.join("pyproject.toml"),
            "[project.scripts]\nsdk = \"sdk.cli:run\"\n",
        )
        .unwrap();

        let old_surface = extract_python_surface(&old).unwrap();
        assert_eq!(
            old_surface.modules,
            vec!["sdk", "sdk.errors", "sdk.legacy", "sdk.models"]
        );
        assert_eq!(
            old_surface.public_exports,
            vec!["sdk.ApiError", "sdk.Book", "sdk.BookAlias"]
        );
        assert!(old_surface.models["Book"].fields["title"].required);
        assert_eq!(
            old_surface.models["Book"].fields["title"].alias.as_deref(),
            Some("bookTitle")
        );
        assert_eq!(old_surface.models["Book"].fields["note"].ty, "None|str");
        assert!(old_surface.models["Book"].has_from_dict);
        assert!(old_surface.models["Book"].has_to_dict);
        assert_eq!(old_surface.exception_classes, vec!["ApiError"]);
        assert_eq!(old_surface.aliases["BookAlias"], "Book");

        let diff = diff_python_dirs(&old, &new).unwrap();
        assert!(diff.is_breaking());
        assert_eq!(diff.missing_modules, vec!["sdk.legacy"]);
        assert_eq!(
            diff.missing_public_exports,
            vec!["sdk.ApiError", "sdk.BookAlias"]
        );
        assert_eq!(diff.model_field_changes.len(), 1, "{diff:?}");
        assert_eq!(diff.model_field_changes[0].field, "title");
        assert_eq!(diff.constructor_changes.len(), 1);
        assert_eq!(diff.missing_from_dict, vec!["Book"]);
        assert_eq!(diff.missing_exception_classes, vec!["ApiError"]);
        assert!(diff
            .alias_changes
            .iter()
            .any(|change| change == "Book.title: bookTitle -> title"));
        assert!(diff
            .package_entry_point_changes
            .iter()
            .any(|change| change.contains("sdk.cli:main -> sdk.cli:run")));

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn python_surface_handles_model_syntax_independent_of_layout() {
        let dir = temp_dir("python-modern-model-layout");
        std::fs::create_dir_all(dir.join("sdk")).unwrap();
        std::fs::write(
            dir.join("sdk/book.py"),
            "from typing import Annotated, Optional, Required, TypedDict\n\nclass Book(BaseModel):\n  title: Annotated[\n    Optional[str],\n    Field(alias = \"bookTitle\"),\n  ] = Field(default = None, alias = \"bookTitle\")  # wire name\n\n  @classmethod\n  def from_dict(cls, data):\n    return cls(**data)\n\n  def to_dict(self):\n    return {}\n\nclass Author(BaseModel):\n\tname: str\n\n@attrs.define\nclass Publisher:\n  name: str = attrs.field()\n  slug: str = attrs.field(factory=str)\n\nclass BookPatch(TypedDict, total=False):\n  title: str\n  id: Required[int]\n\ntype BookAlias = Book\n",
        )
        .unwrap();

        let surface = extract_python_surface(&dir).unwrap();
        let title = &surface.models["Book"].fields["title"];
        assert!(!title.required);
        assert!(title.nullable);
        assert_eq!(title.alias.as_deref(), Some("bookTitle"));
        assert!(surface.models["Book"].has_from_dict);
        assert!(surface.models["Book"].has_to_dict);
        assert_eq!(surface.models["Author"].fields["name"].ty, "str");
        assert!(surface.models["Publisher"].fields["name"].required);
        assert!(!surface.models["Publisher"].fields["slug"].required);
        assert!(!surface.models["BookPatch"].fields["title"].required);
        assert!(surface.models["BookPatch"].fields["id"].required);
        assert_eq!(surface.aliases["BookAlias"], "Book");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn python_surface_uses_setuptools_import_package_mappings() {
        let package_directory = temp_dir("python-package-directory-layout");
        let mapped_root = temp_dir("python-mapped-root-layout");
        let source_root = temp_dir("python-source-root-layout");

        std::fs::create_dir_all(package_directory.join("sdk")).unwrap();
        std::fs::write(
            package_directory.join("sdk/__init__.py"),
            "from .models import Book\n",
        )
        .unwrap();
        std::fs::write(
            package_directory.join("sdk/models.py"),
            "class Book(BaseModel):\n    title: str\n",
        )
        .unwrap();

        std::fs::write(
            mapped_root.join("pyproject.toml"),
            "[tool.setuptools]\npackages = [\"sdk\"]\n\n[tool.setuptools.package-dir]\nsdk = \".\"\n",
        )
        .unwrap();
        std::fs::write(
            mapped_root.join("__init__.py"),
            "from .models import Book\n",
        )
        .unwrap();
        std::fs::write(
            mapped_root.join("models.py"),
            "class Book(BaseModel):\n    title: str\n",
        )
        .unwrap();

        std::fs::create_dir_all(source_root.join("src/sdk")).unwrap();
        std::fs::write(
            source_root.join("pyproject.toml"),
            "[tool.setuptools.package-dir]\n\"\" = \"src\"\n",
        )
        .unwrap();
        std::fs::write(
            source_root.join("src/sdk/__init__.py"),
            "from .models import Book\n",
        )
        .unwrap();
        std::fs::write(
            source_root.join("src/sdk/models.py"),
            "class Book(BaseModel):\n    title: str\n",
        )
        .unwrap();
        std::fs::write(
            source_root.join("build_helper.py"),
            "class NotDistributed(BaseModel):\n    value: str\n",
        )
        .unwrap();

        let expected = extract_python_surface(&package_directory).unwrap();
        for candidate in [&mapped_root, &source_root] {
            let surface = extract_python_surface(candidate).unwrap();
            assert_eq!(surface.modules, vec!["sdk", "sdk.models"]);
            assert_eq!(surface.public_exports, vec!["sdk.Book"]);
            assert_eq!(surface.models, expected.models);
            assert!(!diff_python_dirs(&package_directory, candidate)
                .unwrap()
                .is_breaking());
        }

        let _ = std::fs::remove_dir_all(package_directory);
        let _ = std::fs::remove_dir_all(mapped_root);
        let _ = std::fs::remove_dir_all(source_root);
    }

    #[test]
    fn python_surface_preserves_user_type_identifiers_and_ignores_comments() {
        let old = temp_dir("python-type-name-old");
        let new = temp_dir("python-type-name-new");
        std::fs::write(
            old.join("models.py"),
            "class Book(BaseModel):\n    values: MyList[str]  # old prose\n    owner: mytyping.Owner\n",
        )
        .unwrap();
        std::fs::write(
            new.join("models.py"),
            "class Book(BaseModel):\n    values: Mylist[str]  # new prose\n    owner: myOwner\n",
        )
        .unwrap();

        let old_surface = extract_python_surface(&old).unwrap();
        assert_eq!(
            old_surface.models["Book"].fields["values"].ty,
            "MyList[str]"
        );
        assert_eq!(
            old_surface.models["Book"].fields["owner"].ty,
            "mytyping.Owner"
        );
        let diff = diff_python_dirs(&old, &new).unwrap();
        assert_eq!(diff.model_field_changes.len(), 2, "{diff:?}");

        let equivalent = temp_dir("python-type-name-equivalent");
        std::fs::write(
            equivalent.join("models.py"),
            "class Book(BaseModel):\n    values: MyList[str]  # rewritten prose\n    owner: mytyping.Owner\n",
        )
        .unwrap();
        let comment_only_diff = diff_python_dirs(&old, &equivalent).unwrap();
        assert!(
            !comment_only_diff.is_breaking(),
            "comments are not public API: {comment_only_diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
        let _ = std::fs::remove_dir_all(equivalent);
    }

    #[test]
    fn python_surface_parses_semantically_equivalent_pyproject_entry_points() {
        let table = temp_dir("python-entry-points-table");
        let inline = temp_dir("python-entry-points-inline");
        let invalid = temp_dir("python-entry-points-invalid");
        std::fs::write(
            table.join("pyproject.toml"),
            "[project.scripts]\nsdk = \"sdk.cli:main\"\n\n[project.entry-points.\"sdk.plugins\"]\nbooks = \"sdk.books:plugin\"\n",
        )
        .unwrap();
        std::fs::write(
            inline.join("pyproject.toml"),
            "[project]\nscripts = { sdk = \"sdk.cli:main\" }\nentry-points = { \"sdk.plugins\" = { books = \"sdk.books:plugin\" } }\n",
        )
        .unwrap();
        std::fs::write(
            invalid.join("pyproject.toml"),
            "[project.scripts]\nsdk = 42\n",
        )
        .unwrap();

        let table_surface = extract_python_surface(&table).unwrap();
        let inline_surface = extract_python_surface(&inline).unwrap();
        assert_eq!(
            table_surface.package_entry_points,
            inline_surface.package_entry_points
        );
        assert_eq!(
            table_surface.package_entry_points["project.entry-points.sdk.plugins.books"],
            "sdk.books:plugin"
        );
        assert!(extract_python_surface(&invalid).is_err());

        let _ = std::fs::remove_dir_all(table);
        let _ = std::fs::remove_dir_all(inline);
        let _ = std::fs::remove_dir_all(invalid);
    }

    #[test]
    fn python_surface_preserves_constructor_calling_conventions() {
        let positional = temp_dir("python-constructor-positional");
        let keyword = temp_dir("python-constructor-keyword");
        std::fs::write(
            positional.join("models.py"),
            "class Book:\n    def __init__(self, title: str, /):\n        pass\n",
        )
        .unwrap();
        std::fs::write(
            keyword.join("models.py"),
            "class Book:\n    def __init__(self, *, title: str):\n        pass\n",
        )
        .unwrap();

        let diff = diff_python_dirs(&positional, &keyword).unwrap();
        assert_eq!(diff.constructor_changes.len(), 1, "{diff:?}");
        assert_eq!(diff.constructor_changes[0].old, "(title: str, /)");
        assert_eq!(diff.constructor_changes[0].new, "(*, title: str)");

        let _ = std::fs::remove_dir_all(positional);
        let _ = std::fs::remove_dir_all(keyword);
    }

    #[test]
    fn python_surface_only_reads_real_all_assignments() {
        let dir = temp_dir("python-real-all");
        let empty = temp_dir("python-empty-all");
        let dynamic = temp_dir("python-dynamic-all");
        std::fs::create_dir_all(dir.join("sdk")).unwrap();
        std::fs::create_dir_all(empty.join("sdk")).unwrap();
        std::fs::create_dir_all(dynamic.join("sdk")).unwrap();
        std::fs::write(
            dir.join("sdk/__init__.py"),
            "# __all__ is deliberately omitted\nPRIVATE_VALUES = [\"Private\"]\nfrom .models import Book\n",
        )
        .unwrap();
        std::fs::write(dir.join("sdk/models.py"), "class Book:\n    title: str\n").unwrap();
        std::fs::write(
            empty.join("sdk/__init__.py"),
            "from .models import Book\n__all__ = [\"Legacy\"]\n__all__ = []\n",
        )
        .unwrap();
        std::fs::write(empty.join("sdk/models.py"), "class Book:\n    title: str\n").unwrap();
        std::fs::write(
            dynamic.join("sdk/__init__.py"),
            "__all__ = build_exports()\n",
        )
        .unwrap();

        let surface = extract_python_surface(&dir).unwrap();
        assert_eq!(surface.public_exports, vec!["sdk.Book"]);
        assert!(extract_python_surface(&empty)
            .unwrap()
            .public_exports
            .is_empty());
        assert!(extract_python_surface(&dynamic).is_err());

        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(empty);
        let _ = std::fs::remove_dir_all(dynamic);
    }

    #[test]
    fn python_surface_normalizes_setup_cfg_entry_point_layout() {
        let inline = temp_dir("python-setup-cfg-inline");
        let multiline = temp_dir("python-setup-cfg-multiline");
        std::fs::write(
            inline.join("setup.cfg"),
            "[options.entry_points]\nconsole_scripts = sdk = sdk.cli:main\n",
        )
        .unwrap();
        std::fs::write(
            multiline.join("setup.cfg"),
            "[options.entry_points]\nconsole_scripts =\n    sdk = sdk.cli:main\n",
        )
        .unwrap();

        let inline_surface = extract_python_surface(&inline).unwrap();
        let multiline_surface = extract_python_surface(&multiline).unwrap();
        assert_eq!(
            inline_surface.package_entry_points,
            multiline_surface.package_entry_points
        );

        let _ = std::fs::remove_dir_all(inline);
        let _ = std::fs::remove_dir_all(multiline);
    }

    #[test]
    fn diffs_missing_symbols_and_kind_mismatches() {
        let old = temp_dir("old");
        let new = temp_dir("new");
        for dir in [&old, &new] {
            std::fs::write(
                dir.join("index.ts"),
                "export * from \"./models\";\nexport * from \"./api\";\n",
            )
            .unwrap();
        }
        std::fs::write(
            old.join("models.ts"),
            "export interface Book {\n  title?: string | null;\n  author: Author;\n}\nexport interface Author {\n  name: string;\n}\nexport const Format = {} as const;\nexport type Format = typeof Format[keyof typeof Format];\n",
        )
        .unwrap();
        std::fs::write(old.join("api.ts"), "export interface CreateBookRequest {}\nexport class DefaultApi {\n  async createBook(): Promise<AxiosResponse<Book>> { return response; }\n}\nexport const DefaultApiFactory = function () {\n  return {\n    createBook(): Promise<AxiosResponse<Book>> { return api.createBook(); },\n  };\n};\n").unwrap();
        std::fs::create_dir_all(old.join("docs")).unwrap();
        std::fs::write(old.join("docs/Book.md"), "# Book\n").unwrap();
        std::fs::write(old.join("docs/BooksApi.md"), "# BooksApi\n").unwrap();
        std::fs::write(
            new.join("models.ts"),
            "export interface Book {\n  title: string;\n}\nexport type Format = \"hardcover\";\n",
        )
        .unwrap();
        std::fs::write(
            new.join("api.ts"),
            "export class DefaultApi {\n  async createBook(): Promise<Book> { return book; }\n}\n",
        )
        .unwrap();

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert!(diff.is_breaking());
        assert_eq!(diff.export_kind_mismatches[0].symbol, "Format");
        assert_eq!(diff.missing_api_factories, vec!["DefaultApiFactory"]);
        assert_eq!(diff.missing_request_aliases, vec!["CreateBookRequest"]);
        assert!(diff
            .missing_interface_properties
            .iter()
            .any(|missing| missing.interface == "Book" && missing.property == "author"));
        assert_eq!(diff.interface_property_changes[0].property, "title");
        assert!(diff.interface_property_changes[0].old.optional);
        assert!(!diff.interface_property_changes[0].new.optional);
        assert!(diff.interface_property_changes[0].old.nullable);
        assert!(!diff.interface_property_changes[0].new.nullable);
        assert_eq!(diff.interface_optional_to_required[0].property, "title");
        assert!(diff.interface_required_to_optional.is_empty());
        assert_eq!(diff.interface_nullable_changes[0].property, "title");
        assert!(
            diff.interface_type_changes.is_empty(),
            "nullability-only type text changes should not be reported as base type changes"
        );
        assert_eq!(
            diff.operation_return_type_changes[0].old,
            "Promise<AxiosResponse<Book>>"
        );
        assert_eq!(diff.operation_return_type_changes[0].new, "Promise<Book>");
        assert_eq!(
            diff.operation_signature_changes[0].old,
            "async createBook(): Promise<AxiosResponse<Book>>"
        );
        assert_eq!(
            diff.operation_signature_changes[0].new,
            "async createBook(): Promise<Book>"
        );
        assert_eq!(
            diff.missing_docs,
            vec!["docs/Book.md".to_string(), "docs/BooksApi.md".to_string()]
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_diff_normalizes_multiline_interface_property_types() {
        let old = temp_dir("ts-old-multiline-property");
        let new = temp_dir("ts-new-multiline-property");
        for dir in [&old, &new] {
            std::fs::write(
                dir.join("index.ts"),
                "export interface BranchApiInterface extends LegacyApiInterface {\n  /** Delete a branch. */\n  ingestBranchesBranchIdDelete: LegacyApiMethod<\n    models.CommandMessage,\n    [request: { branchId: string }]\n  >;\n}\n",
            )
            .unwrap();
        }

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert!(
            diff.interface_type_changes.is_empty(),
            "unchanged multiline property type should not diff: {:?}",
            diff.interface_type_changes
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn typescript_diff_still_reports_real_multiline_interface_property_type_changes() {
        let old = temp_dir("ts-old-real-multiline-property");
        let new = temp_dir("ts-new-real-multiline-property");
        std::fs::write(
            old.join("index.ts"),
            "export interface BranchApiInterface {\n  ingestBranchesBranchIdDelete: LegacyApiMethod<\n    models.CommandMessage,\n    [request: { branchId: string }]\n  >;\n}\n",
        )
        .unwrap();
        std::fs::write(
            new.join("index.ts"),
            "export interface BranchApiInterface {\n  ingestBranchesBranchIdDelete: LegacyApiMethod<\n    models.OtherMessage,\n    [request: { branchId: string }]\n  >;\n}\n",
        )
        .unwrap();

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert_eq!(diff.interface_type_changes.len(), 1);
        assert_eq!(
            diff.interface_type_changes[0].property,
            "ingestBranchesBranchIdDelete"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_diff_catches_missing_symbols_and_signature_changes() {
        let old = temp_dir("go-old");
        let new = temp_dir("go-new");
        std::fs::write(old.join("go.mod"), "module example.com/old\n\ngo 1.23\n").unwrap();
        std::fs::write(new.join("go.mod"), "module example.com/new\n\ngo 1.23\n").unwrap();
        std::fs::write(old.join("README.md"), "# Old SDK\n").unwrap();
        std::fs::write(
            old.join("client.go"),
            "package sdk\n\ntype Configuration struct{}\ntype APIClient struct{}\ntype BooksAPIService service\nfunc NewConfiguration() *Configuration { return nil }\nfunc (r ApiGetBookRequest) Execute() (*Book, error) { return nil, nil }\n",
        )
        .unwrap();
        std::fs::write(
            new.join("client.go"),
            "package sdk\n\ntype Configuration struct{}\ntype APIClient struct{}\nfunc NewConfiguration(baseURL string) *Configuration { return nil }\nfunc (r ApiGetBookRequest) Execute(ctx context.Context) (*Book, error) { return nil, nil }\n",
        )
        .unwrap();

        let diff = diff_go_dirs(&old, &new).unwrap();
        assert!(diff.is_breaking());
        assert_eq!(diff.missing_exported_types, vec!["BooksAPIService"]);
        assert_eq!(diff.missing_exported_methods, Vec::<String>::new());
        assert_eq!(
            diff.exported_function_signature_changes[0].symbol,
            "NewConfiguration"
        );
        assert_eq!(
            diff.exported_method_signature_changes[0].symbol,
            "ApiGetBookRequest.Execute"
        );
        assert_eq!(diff.missing_docs, vec!["README.md"]);
        assert_eq!(
            diff.package_metadata_changes,
            vec!["module: example.com/old -> example.com/new"]
        );
    }

    #[test]
    fn go_diff_catches_exported_type_shape_changes() {
        let old = temp_dir("go-type-shape-old");
        let new = temp_dir("go-type-shape-new");
        std::fs::write(old.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        std::fs::write(new.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        std::fs::write(
            old.join("models.go"),
            r#"package sdk

type (
    Book struct {
        Title string `json:"title"`
        Tags []string `json:"tags,omitempty"`
    }
    Status = string
    Reader interface {
        Read([]byte) (int, error)
    }
    internal struct { Value string }
)
"#,
        )
        .unwrap();
        std::fs::write(
            new.join("models.go"),
            r#"package sdk

type (
    Book struct {
        Title int `json:"title"`
        Tags []string `json:"tags,omitempty"`
    }
    Status = int
    Reader interface {
        Read([]byte) error
    }
    internal struct { Value int }
)
"#,
        )
        .unwrap();

        let old_surface = extract_go_surface(&old).unwrap();
        assert_eq!(old_surface.exported_types, vec!["Book", "Reader", "Status"]);
        assert!(!old_surface
            .exported_type_declarations
            .contains_key("internal"));
        let diff = diff_go_dirs(&old, &new).unwrap();
        assert_eq!(
            diff.exported_type_changes
                .iter()
                .map(|change| change.symbol.as_str())
                .collect::<Vec<_>>(),
            vec!["Book", "Reader", "Status"]
        );
        assert!(diff.is_breaking());

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_type_shape_comparison_ignores_layout_and_comments() {
        let old = temp_dir("go-type-layout-old");
        let new = temp_dir("go-type-layout-new");
        for dir in [&old, &new] {
            std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        }
        std::fs::write(
            old.join("models.go"),
            r#"package sdk
type Book struct {
    Title string `json:"title"` // public title
    Tags []string `json:"tags,omitempty"`
}
"#,
        )
        .unwrap();
        std::fs::write(
            new.join("models.go"),
            r#"package sdk
type Book struct { Title string `json:"title"`; Tags []string `json:"tags,omitempty"` }
"#,
        )
        .unwrap();

        let diff = diff_go_dirs(&old, &new).unwrap();
        assert!(
            diff.exported_type_changes.is_empty(),
            "formatting-only type drift was reported: {diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_diff_ignores_parameter_and_result_names() {
        let old = temp_dir("go-old-param-names");
        let new = temp_dir("go-new-param-names");
        std::fs::write(old.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        std::fs::write(new.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        std::fs::write(
            old.join("client.go"),
            "package sdk\n\ntype Configuration struct{}\nfunc NewConfiguration(baseURL string, priceID any) (*Configuration, error) { return nil, nil }\nfunc (r ApiGetBookRequest) PriceId(priceID any) ApiGetBookRequest { return r }\nfunc (r ApiGetBookRequest) Execute() (value *Book, resp *http.Response, err error) { return nil, nil, nil }\n",
        )
        .unwrap();
        std::fs::write(
            new.join("client.go"),
            "package sdk\n\ntype Configuration struct{}\nfunc NewConfiguration(rawURL string, priceId any) (*Configuration, error) { return nil, nil }\nfunc (request ApiGetBookRequest) PriceId(priceId any) ApiGetBookRequest { return request }\nfunc (request ApiGetBookRequest) Execute() (*Book, *http.Response, error) { return nil, nil, nil }\n",
        )
        .unwrap();

        let diff = diff_go_dirs(&old, &new).unwrap();
        assert!(
            diff.exported_function_signature_changes.is_empty(),
            "parameter names should be ignored for functions: {:?}",
            diff.exported_function_signature_changes
        );
        assert!(
            diff.exported_method_signature_changes.is_empty(),
            "parameter/result names should be ignored for methods: {:?}",
            diff.exported_method_signature_changes
        );
        assert!(!diff.is_breaking(), "unexpected Go surface diff: {diff:?}");

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_diff_ignores_names_in_interface_methods_and_function_types() {
        let old = temp_dir("go-old-type-param-names");
        let new = temp_dir("go-new-type-param-names");
        for dir in [&old, &new] {
            std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        }
        std::fs::write(
            old.join("types.go"),
            r"package sdk
type Reader interface {
    Read(buffer []byte) (count int, err error)
    Transform(callback func(input string) (output int, err error)) (done bool)
}
type Handler func(context string, request *Request) (response *Response, err error)
",
        )
        .unwrap();
        std::fs::write(
            new.join("types.go"),
            r"package sdk
type Reader interface {
    Read(payload []byte) (written int, failure error)
    Transform(fn func(value string) (result int, failure error)) (complete bool)
}
type Handler func(ctx string, req *Request) (result *Response, failure error)
",
        )
        .unwrap();

        let diff = diff_go_dirs(&old, &new).unwrap();

        assert!(
            diff.exported_type_changes.is_empty(),
            "parameter and result names are not part of Go type identity: {diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_diff_parses_multiline_signatures_and_grouped_parameter_names() {
        let old = temp_dir("go-old-multiline-signature");
        let new = temp_dir("go-new-multiline-signature");
        let equivalent = temp_dir("go-equivalent-grouped-signature");
        for dir in [&old, &new, &equivalent] {
            std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        }
        std::fs::write(
            old.join("client.go"),
            "package sdk\n\nfunc NewClient(\n    host, token string,\n    retries int,\n) (*Client, error) {\n    return nil, nil\n}\n",
        )
        .unwrap();
        std::fs::write(
            new.join("client.go"),
            "package sdk\n\nfunc NewClient(\n    baseURL, bearer string,\n    retries string,\n) (*Client, error) {\n    return nil, nil\n}\n",
        )
        .unwrap();
        std::fs::write(
            equivalent.join("client.go"),
            "package sdk\n\nfunc NewClient(baseURL string, bearer string, retryCount int) (*Client, error) {\n    return nil, nil\n}\n",
        )
        .unwrap();

        let changed = diff_go_dirs(&old, &new).unwrap();
        assert_eq!(changed.exported_function_signature_changes.len(), 1);
        let same = diff_go_dirs(&old, &equivalent).unwrap();
        assert!(
            same.exported_function_signature_changes.is_empty(),
            "grouped parameter names are not part of the Go function type: {same:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
        let _ = std::fs::remove_dir_all(equivalent);
    }

    #[test]
    fn go_surface_excludes_test_only_exports() {
        let old = temp_dir("go-test-exports-old");
        let new = temp_dir("go-test-exports-new");
        for dir in [&old, &new] {
            std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
            std::fs::write(
                dir.join("client.go"),
                "package sdk\n\ntype Client struct{}\n",
            )
            .unwrap();
        }
        std::fs::write(
            old.join("client_test.go"),
            "package sdk\n\nfunc LegacyTestHelper() {}\n",
        )
        .unwrap();

        let diff = diff_go_dirs(&old, &new).unwrap();
        assert!(
            !diff.is_breaking(),
            "test helpers are not shipped API: {diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_surface_namespaces_symbols_from_subpackages() {
        let old = temp_dir("go-subpackage-symbols-old");
        let new = temp_dir("go-subpackage-symbols-new");
        for dir in [&old, &new] {
            std::fs::create_dir_all(dir.join("models")).unwrap();
            std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
            std::fs::write(
                dir.join("client.go"),
                "package sdk\n\ntype Client struct { Value string }\n",
            )
            .unwrap();
        }
        std::fs::write(
            old.join("models/client.go"),
            "package models\n\ntype Client struct { Value string }\nfunc NewClient(value string) *Client { return nil }\nfunc (c Client) ValueOrDefault() string { return c.Value }\n",
        )
        .unwrap();
        std::fs::write(
            new.join("models/client.go"),
            "package models\n\ntype Client struct { Value int }\nfunc NewClient(value int) *Client { return nil }\nfunc (c Client) ValueOrDefault() int { return c.Value }\n",
        )
        .unwrap();

        let old_surface = extract_go_surface(&old).unwrap();
        assert_eq!(old_surface.exported_types, vec!["Client", "models.Client"]);
        assert!(old_surface
            .exported_functions
            .contains_key("models.NewClient"));
        assert!(old_surface
            .exported_methods
            .contains_key("models.Client.ValueOrDefault"));

        let diff = diff_go_dirs(&old, &new).unwrap();
        assert_eq!(diff.exported_type_changes[0].symbol, "models.Client");
        assert_eq!(
            diff.exported_function_signature_changes[0].symbol,
            "models.NewClient"
        );
        assert_eq!(
            diff.exported_method_signature_changes[0].symbol,
            "models.Client.ValueOrDefault"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_surface_excludes_non_importable_package_directories() {
        let old = temp_dir("go-private-packages-old");
        let new = temp_dir("go-private-packages-new");
        for dir in [&old, &new] {
            std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
            std::fs::write(
                dir.join("client.go"),
                "package sdk\n\ntype Client struct{}\n",
            )
            .unwrap();
        }
        for private_dir in ["internal", "testdata", "_scratch", ".generated"] {
            let path = old.join(private_dir);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join("legacy.go"),
                "package private\n\ntype Legacy struct{}\nfunc LegacyHelper() {}\n",
            )
            .unwrap();
        }

        let old_surface = extract_go_surface(&old).unwrap();
        assert_eq!(old_surface.exported_types, vec!["Client"]);
        let diff = diff_go_dirs(&old, &new).unwrap();
        assert!(
            !diff.is_breaking(),
            "Go-ignored and internal packages are not public SDK surface: {diff:?}"
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_surface_parses_bodyless_functions_and_comments_as_whitespace() {
        let old = temp_dir("go-bodyless-old");
        let new = temp_dir("go-bodyless-new");
        for dir in [&old, &new] {
            std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n").unwrap();
        }
        std::fs::write(
            old.join("assembly.go"),
            "package sdk\n\nfunc/* generated */External(value int)\nfunc Stable() {}\n",
        )
        .unwrap();
        std::fs::write(
            new.join("assembly.go"),
            "package sdk\n\nfunc External(value string)\nfunc Stable() {}\n",
        )
        .unwrap();

        let diff = diff_go_dirs(&old, &new).unwrap();
        assert_eq!(
            diff.exported_function_signature_changes.len(),
            1,
            "{diff:?}"
        );
        assert_eq!(
            diff.exported_function_signature_changes[0].symbol,
            "External"
        );
        assert!(diff.missing_exported_functions.is_empty(), "{diff:?}");

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[cfg(unix)]
    #[test]
    fn sdk_surface_walk_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let dir = temp_dir("ts-symlink-cycle");
        std::fs::write(dir.join("index.ts"), "export interface Book {}\n").unwrap();
        symlink(&dir, dir.join("loop")).unwrap();

        let error = extract_typescript_surface(&dir).unwrap_err().to_string();
        assert!(error.contains("symbolic link"), "{error}");

        let _ = std::fs::remove_file(dir.join("loop"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn go_contract_allows_selected_drift_and_reports_stale_allowances() {
        let old = temp_dir("go-contract-old");
        let new = temp_dir("go-contract-new");
        std::fs::write(old.join("go.mod"), "module example.com/old\n\ngo 1.23\n").unwrap();
        std::fs::write(new.join("go.mod"), "module example.com/new\n\ngo 1.23\n").unwrap();
        std::fs::write(old.join("README.md"), "# Old SDK\n").unwrap();
        std::fs::write(
            old.join("client.go"),
            "package sdk\n\ntype Configuration struct{}\ntype BooksAPIService service\nfunc NewConfiguration() *Configuration { return nil }\nfunc (r ApiGetBookRequest) Execute() (*Book, error) { return nil, nil }\n",
        )
        .unwrap();
        std::fs::write(
            new.join("client.go"),
            "package sdk\n\ntype Configuration struct{}\nfunc NewConfiguration(baseURL string) *Configuration { return nil }\nfunc (r ApiGetBookRequest) Execute(ctx context.Context) (*Book, error) { return nil, nil }\n",
        )
        .unwrap();

        let old_surface = extract_go_surface(&old).unwrap();
        let new_surface = extract_go_surface(&new).unwrap();
        let diff = diff_go_surfaces(&old_surface, &new_surface);
        let contract = GoCompatibilityContract {
            require_exported_types: vec!["Configuration".to_string()],
            allow_missing_exported_types: vec![
                "BooksAPIService".to_string(),
                "StaleType".to_string(),
            ],
            allow_exported_function_signature_changes: vec!["NewConfiguration".to_string()],
            allow_exported_method_signature_changes: vec!["ApiGetBookRequest.Execute".to_string()],
            allow_missing_docs: vec!["README.md".to_string()],
            allow_package_metadata_changes: vec![
                "module: example.com/old -> example.com/new".to_string()
            ],
            ..GoCompatibilityContract::default()
        };

        let evaluation = evaluate_go_contract(&contract, &diff, &new_surface);
        assert!(!evaluation.breaking, "{evaluation:?}");
        assert_eq!(
            evaluation.stale_allowances,
            vec!["go.allow_missing_exported_types: StaleType".to_string()]
        );

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }

    #[test]
    fn go_contract_fails_missing_required_and_unapproved_drift() {
        let old = GoSurface {
            exported_types: vec!["Legacy".to_string()],
            exported_functions: BTreeMap::from([(
                "NewClient".to_string(),
                "func NewClient()".to_string(),
            )]),
            ..GoSurface::default()
        };
        let new = GoSurface::default();
        let diff = diff_go_surfaces(&old, &new);
        let contract = GoCompatibilityContract {
            require_exported_types: vec!["Required".to_string()],
            ..GoCompatibilityContract::default()
        };

        let evaluation = evaluate_go_contract(&contract, &diff, &new);
        assert!(evaluation.breaking);
        assert_eq!(
            evaluation.missing_required,
            vec!["go.require_exported_types: Required".to_string()]
        );
        assert_eq!(
            evaluation.unapproved_diff.missing_exported_types,
            vec!["Legacy".to_string()]
        );
    }

    #[test]
    fn go_contract_can_allow_one_exported_type_change() {
        let old = GoSurface {
            exported_types: vec!["Book".to_string()],
            exported_type_declarations: BTreeMap::from([(
                "Book".to_string(),
                "type Book struct{Title string}".to_string(),
            )]),
            ..GoSurface::default()
        };
        let new = GoSurface {
            exported_types: vec!["Book".to_string()],
            exported_type_declarations: BTreeMap::from([(
                "Book".to_string(),
                "type Book struct{Title int}".to_string(),
            )]),
            ..GoSurface::default()
        };
        let diff = diff_go_surfaces(&old, &new);
        let evaluation = evaluate_go_contract(
            &GoCompatibilityContract {
                allow_exported_type_changes: vec!["Book".to_string()],
                ..GoCompatibilityContract::default()
            },
            &diff,
            &new,
        );

        assert!(!evaluation.breaking, "{evaluation:?}");
        assert!(evaluation.unapproved_diff.exported_type_changes.is_empty());
        assert!(evaluation.stale_allowances.is_empty());
    }

    #[test]
    fn typescript_contract_allows_selected_drift_and_reports_stale_allowances() {
        let old = TypeScriptSurface {
            root_exports: BTreeMap::from([
                ("Book".to_string(), TsExportKind::Type),
                ("Format".to_string(), TsExportKind::Both),
            ]),
            model_exports: BTreeMap::from([("Book".to_string(), TsExportKind::Type)]),
            api_classes: vec!["DefaultApi".to_string()],
            request_aliases: vec!["CreateBookRequest".to_string()],
            interface_properties: BTreeMap::from([(
                "Book".to_string(),
                BTreeMap::from([(
                    "title".to_string(),
                    TsInterfaceProperty {
                        optional: true,
                        nullable: true,
                        ty: "string | null".to_string(),
                    },
                )]),
            )]),
            type_declarations: BTreeMap::from([(
                "BookFormat".to_string(),
                vec!["type BookFormat=\"paperback\" | \"hardcover\"".to_string()],
            )]),
            operation_return_types: BTreeMap::from([(
                "DefaultApi.createBook".to_string(),
                "Promise<AxiosResponse<Book>>".to_string(),
            )]),
            operation_signatures: BTreeMap::from([(
                "DefaultApi.createBook".to_string(),
                "async createBook(): Promise<AxiosResponse<Book>>".to_string(),
            )]),
            ..TypeScriptSurface::default()
        };
        let new = TypeScriptSurface {
            root_exports: BTreeMap::from([
                ("Book".to_string(), TsExportKind::Type),
                ("Format".to_string(), TsExportKind::Type),
            ]),
            model_exports: BTreeMap::from([("Book".to_string(), TsExportKind::Type)]),
            api_classes: vec!["DefaultApi".to_string()],
            interface_properties: BTreeMap::from([(
                "Book".to_string(),
                BTreeMap::from([(
                    "title".to_string(),
                    TsInterfaceProperty {
                        optional: false,
                        nullable: false,
                        ty: "string".to_string(),
                    },
                )]),
            )]),
            type_declarations: BTreeMap::from([(
                "BookFormat".to_string(),
                vec!["type BookFormat=number".to_string()],
            )]),
            operation_return_types: BTreeMap::from([(
                "DefaultApi.createBook".to_string(),
                "Promise<Book>".to_string(),
            )]),
            operation_signatures: BTreeMap::from([(
                "DefaultApi.createBook".to_string(),
                "async createBook(): Promise<Book>".to_string(),
            )]),
            ..TypeScriptSurface::default()
        };
        let diff = diff_typescript_surfaces(&old, &new);
        let contract = TypeScriptCompatibilityContract {
            require_root_exports: vec!["Book".to_string()],
            allow_missing_request_aliases: vec![
                "CreateBookRequest".to_string(),
                "StaleRequest".to_string(),
            ],
            allow_interface_property_changes: vec!["Book.title".to_string()],
            allow_type_declaration_changes: vec!["BookFormat".to_string()],
            allow_operation_return_type_changes: vec!["DefaultApi.createBook".to_string()],
            allow_operation_signature_changes: vec!["DefaultApi.createBook".to_string()],
            allow_export_kind_mismatches: vec!["Format".to_string()],
            ..TypeScriptCompatibilityContract::default()
        };

        let evaluation = evaluate_typescript_contract(&contract, &diff, &new);
        assert!(!evaluation.breaking, "{evaluation:?}");
        assert_eq!(
            evaluation.stale_allowances,
            vec!["typescript.allow_missing_request_aliases: StaleRequest".to_string()]
        );
    }

    #[test]
    fn compat_suggestions_include_high_confidence_snippets() {
        let go = GoSurfaceDiff {
            missing_exported_methods: vec!["ApiCreateBookRequest.Book".to_string()],
            exported_method_signature_changes: vec![
                GoSignatureChange {
                    symbol: "ApiListBooksRequest.Execute".to_string(),
                    old: "func (r ApiListBooksRequest) Execute() (*http.Response, error)"
                        .to_string(),
                    new: "func (r ApiListBooksRequest) Execute() (*Book, *http.Response, error)"
                        .to_string(),
                },
                GoSignatureChange {
                    symbol: "ApiListBooksRequest.PageSize".to_string(),
                    old: "func (r ApiListBooksRequest) PageSize(pageSize any) ApiListBooksRequest"
                        .to_string(),
                    new:
                        "func (r ApiListBooksRequest) PageSize(pageSize int64) ApiListBooksRequest"
                            .to_string(),
                },
            ],
            ..GoSurfaceDiff::default()
        };
        let go_suggestions = suggest_go_compat(&go);
        assert!(go_suggestions
            .iter()
            .any(|suggestion| suggestion.contains("GoExecuteCompatibility")));
        assert!(go_suggestions
            .iter()
            .any(|suggestion| suggestion.contains("GoQuerySetterArgumentPolicy")));
        assert!(go_suggestions
            .iter()
            .any(|suggestion| suggestion.contains("GoRequestBuilderAliases")));

        let ts = super::TypeScriptSurfaceDiff {
            export_kind_mismatches: vec![TsExportKindMismatch {
                symbol: "Format".to_string(),
                old: TsExportKind::Both,
                new: TsExportKind::Type,
            }],
            interface_nullable_changes: vec![super::TsInterfacePropertyChange {
                interface: "Book".to_string(),
                property: "title".to_string(),
                old: TsInterfaceProperty {
                    optional: true,
                    nullable: true,
                    ty: "string | null".to_string(),
                },
                new: TsInterfaceProperty {
                    optional: true,
                    nullable: false,
                    ty: "string".to_string(),
                },
            }],
            operation_return_type_changes: vec![super::TsOperationReturnTypeChange {
                operation: "DefaultApi.createBook".to_string(),
                old: "Promise<AxiosResponse<Book>>".to_string(),
                new: "Promise<Book>".to_string(),
            }],
            ..super::TypeScriptSurfaceDiff::default()
        };
        let ts_suggestions = suggest_typescript_compat(&ts);
        assert!(ts_suggestions
            .iter()
            .any(|suggestion| suggestion.contains("TsCompatibility::OpenApiGenerator")));
        assert!(ts_suggestions
            .iter()
            .any(|suggestion| suggestion.contains("TsNullablePolicy")));
    }
}
