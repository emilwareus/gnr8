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
    let mut exported_functions = BTreeMap::new();
    let mut exported_methods = BTreeMap::new();
    for rel in &files {
        let text = read_to_string(dir.join(rel))?;
        let parsed = parse_go_file(&text);
        exported_types.extend(parsed.types);
        exported_functions.extend(parsed.functions);
        exported_methods.extend(parsed.methods);
    }

    Ok(GoSurface {
        exported_types: exported_types.into_iter().collect(),
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

    let modules = files.iter().map(|path| python_module_name(path)).collect();
    let mut public_exports = BTreeSet::new();
    let mut models = BTreeMap::new();
    let mut exception_classes = BTreeSet::new();
    let mut aliases = BTreeMap::new();

    for rel in &files {
        let text = read_to_string(dir.join(rel))?;
        if rel.file_name().and_then(|name| name.to_str()) == Some("__init__.py") {
            public_exports.extend(parse_python_package_exports(rel, &text));
        }
        let parsed = parse_python_file(&text, is_python_model_file(rel));
        exception_classes.extend(parsed.exceptions);
        aliases.extend(parsed.aliases);
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
    let mut api_classes = BTreeSet::new();
    let mut api_factories = BTreeSet::new();
    let mut operation_methods = BTreeSet::new();
    let mut request_aliases = BTreeSet::new();
    let mut interface_properties = BTreeMap::new();
    let mut operation_return_types = BTreeMap::new();
    let mut operation_signatures = BTreeMap::new();

    for rel in &files {
        let text = read_to_string(dir.join(rel))?;
        let parsed = parse_ts_file(&text);
        merge_exports(&mut all_exports, &parsed.exports);
        merge_interface_properties(&mut interface_properties, parsed.interface_properties);
        merge_operation_return_types(&mut operation_return_types, parsed.operation_return_types);
        operation_signatures.extend(parsed.operation_signatures);
        if is_model_file(rel) {
            merge_exports(&mut model_exports, &parsed.exports);
        }
        api_classes.extend(parsed.api_classes);
        api_factories.extend(parsed.api_factories);
        operation_methods.extend(parsed.operation_methods);
        request_aliases.extend(parsed.request_aliases);
    }

    let root_exports = extract_root_exports(dir, &files, &all_exports)?;
    Ok(TypeScriptSurface {
        root_exports,
        model_exports,
        api_classes: api_classes.into_iter().collect(),
        api_factories: api_factories.into_iter().collect(),
        operation_methods: operation_methods.into_iter().collect(),
        request_aliases: request_aliases.into_iter().collect(),
        interface_properties,
        operation_return_types,
        operation_signatures,
        package_entry_points: package_entry_points(dir)?,
        docs: doc_files(dir)?,
    })
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
    api_classes: Vec<String>,
    api_factories: Vec<String>,
    operation_methods: Vec<String>,
    request_aliases: Vec<String>,
    interface_properties: BTreeMap<String, BTreeMap<String, TsInterfaceProperty>>,
    operation_return_types: BTreeMap<String, String>,
    operation_signatures: BTreeMap<String, String>,
}

#[derive(Default)]
struct TsInterfaceState {
    name: String,
    depth: i32,
    property: String,
}

#[expect(
    clippy::too_many_lines,
    reason = "single-pass parser state is easier to audit together"
)]
fn parse_ts_file(text: &str) -> ParsedTsFile {
    let mut parsed = ParsedTsFile::default();
    let mut current_api_class: Option<(String, i32)> = None;
    let mut current_api_factory: Option<(String, i32)> = None;
    let mut current_interface: Option<TsInterfaceState> = None;
    for raw in text.lines() {
        let line = raw.trim();
        let mut starts_api_class = false;
        let mut starts_api_factory = false;
        let mut starts_interface = false;
        if let Some(name) = strip_export_decl(line, "interface") {
            add_export(&mut parsed.exports, name, TsExportKind::Type);
            if name.ends_with("Request") {
                parsed.request_aliases.push(name.to_string());
            }
            parsed
                .interface_properties
                .entry(name.to_string())
                .or_default();
            let depth = brace_delta(line);
            if depth > 0 {
                current_interface = Some(TsInterfaceState {
                    name: name.to_string(),
                    depth,
                    property: String::new(),
                });
                starts_interface = true;
            }
        } else if let Some(name) = strip_export_decl(line, "type") {
            add_export(&mut parsed.exports, name, TsExportKind::Type);
            if name.ends_with("Request") {
                parsed.request_aliases.push(name.to_string());
            }
        } else if let Some(name) = strip_export_decl(line, "class") {
            add_export(&mut parsed.exports, name, TsExportKind::Value);
            if name.ends_with("Api") {
                current_api_class = Some((name.to_string(), brace_delta(line).max(1)));
                starts_api_class = true;
                parsed.api_classes.push(name.to_string());
            }
        } else if let Some(name) = strip_export_decl(line, "const") {
            add_export(&mut parsed.exports, name, TsExportKind::Value);
            if name.ends_with("ApiFactory") {
                parsed.api_factories.push(name.to_string());
                current_api_factory = Some((name.to_string(), brace_delta(line).max(1)));
                starts_api_factory = true;
            }
        } else if let Some(exports) = line.strip_prefix("export type {") {
            parse_export_list(exports, TsExportKind::Type, &mut parsed.exports);
        } else if let Some(exports) = line.strip_prefix("export {") {
            parse_export_list(exports, TsExportKind::Value, &mut parsed.exports);
        }

        let mut close_api_class = false;
        if let Some((class_name, depth)) = &mut current_api_class {
            if !starts_api_class {
                if let Some((method, return_ty)) = parse_async_method_signature(line) {
                    let key = format!("{class_name}.{method}");
                    parsed.operation_methods.push(key.clone());
                    if let Some(return_ty) = return_ty {
                        parsed.operation_return_types.insert(key.clone(), return_ty);
                    }
                    parsed
                        .operation_signatures
                        .insert(key, normalize_ts_signature(line));
                }
                *depth += brace_delta(line);
            }
            if *depth <= 0 {
                close_api_class = true;
            }
        }
        if close_api_class {
            current_api_class = None;
        }

        let mut close_api_factory = false;
        if let Some((factory_name, depth)) = &mut current_api_factory {
            if !starts_api_factory {
                if let Some((method, Some(return_ty))) = parse_method_signature(line) {
                    let key = format!("{factory_name}.{method}");
                    parsed.operation_return_types.insert(key.clone(), return_ty);
                    parsed
                        .operation_signatures
                        .insert(key, normalize_ts_signature(line));
                }
                *depth += brace_delta(line);
            }
            if *depth <= 0 {
                close_api_factory = true;
            }
        }
        if close_api_factory {
            current_api_factory = None;
        }

        let mut close_interface = false;
        if let Some(interface) = &mut current_interface {
            if !starts_interface {
                collect_interface_property(line, interface, &mut parsed.interface_properties);
                interface.depth += brace_delta(line);
            }
            if interface.depth <= 0 {
                close_interface = true;
            }
        }
        if close_interface {
            current_interface = None;
        }
    }
    parsed
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

fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |delta, ch| match ch {
        '{' => delta + 1,
        '}' => delta - 1,
        _ => delta,
    })
}

fn strip_export_decl<'a>(line: &'a str, kind: &str) -> Option<&'a str> {
    let rest = line.strip_prefix("export ")?;
    let rest = rest.strip_prefix(kind)?;
    let rest = rest.trim_start();
    ident_prefix(rest)
}

fn parse_async_method_signature(line: &str) -> Option<(&str, Option<String>)> {
    let rest = line.strip_prefix("async ")?;
    parse_method_signature(rest)
}

fn parse_method_signature(line: &str) -> Option<(&str, Option<String>)> {
    let method = ident_prefix(line)?;
    let rest = line.get(method.len()..)?.trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let return_ty = method_return_type(rest);
    Some((method, return_ty))
}

fn method_return_type(signature_tail: &str) -> Option<String> {
    let close = signature_tail.find(')')?;
    let after_paren = &signature_tail[close + 1..];
    let rest = after_paren.trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let end = rest
        .find('{')
        .or_else(|| rest.find(';'))
        .unwrap_or(rest.len());
    let ty = rest[..end].trim();
    (!ty.is_empty()).then(|| normalize_ts_type(ty))
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
    ty.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_ts_signature(signature: &str) -> String {
    let signature = signature
        .split_once('{')
        .map_or(signature, |(signature, _)| signature)
        .trim()
        .trim_end_matches(';')
        .trim_end_matches(',')
        .trim();
    signature.split_whitespace().collect::<Vec<_>>().join(" ")
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

fn parse_export_list(exports: &str, kind: TsExportKind, into: &mut BTreeMap<String, TsExportKind>) {
    let Some((list, _)) = exports.split_once('}') else {
        return;
    };
    for part in list.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let name = part
            .split_whitespace()
            .collect::<Vec<_>>()
            .rsplit(|token| *token == "as")
            .next()
            .and_then(|tokens| tokens.last())
            .copied()
            .unwrap_or(part);
        if let Some(name) = ident_prefix(name) {
            add_export(into, name, kind);
        }
    }
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

fn merge_operation_return_types(
    into: &mut BTreeMap<String, String>,
    return_types: BTreeMap<String, String>,
) {
    into.extend(return_types);
}

fn extract_root_exports(
    dir: &Path,
    files: &[PathBuf],
    all_exports: &BTreeMap<String, TsExportKind>,
) -> Result<BTreeMap<String, TsExportKind>, CoreError> {
    let index = Path::new("index.ts");
    if !dir.join(index).exists() {
        return Ok(all_exports.clone());
    }
    let mut cache = BTreeMap::new();
    let mut stack = Vec::new();
    let root = exports_from_file(dir, index, &mut cache, &mut stack)?;
    if root.is_empty() && files.iter().any(|path| path == Path::new("index.ts")) {
        return Ok(all_exports.clone());
    }
    Ok(root)
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
    let mut exports = parsed.exports;
    let base = rel.parent().unwrap_or_else(|| Path::new(""));
    for line in text.lines().map(str::trim) {
        if let Some(spec) = export_star_module_spec(line) {
            if let Some(target) = resolve_ts_module(dir, base, spec) {
                let nested = exports_from_file(dir, &target, cache, stack)?;
                merge_exports(&mut exports, &nested);
            }
        }
    }

    stack.pop();
    cache.insert(rel.to_path_buf(), exports.clone());
    Ok(exports)
}

fn export_star_module_spec(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("export * from ")?;
    quoted_module_spec(rest)
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
        joined.join("index.ts"),
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
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("node_modules") {
                continue;
            }
            collect_ts_files(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("ts") {
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
    is_model: bool,
    is_exception: bool,
    model: PythonModelSurface,
}

fn parse_python_file(text: &str, model_file: bool) -> ParsedPythonFile {
    let lines: Vec<&str> = text.lines().collect();
    let mut parsed = ParsedPythonFile::default();
    let mut current: Option<PythonClassState> = None;
    let mut index = 0;
    while index < lines.len() {
        let raw = lines[index];
        let trimmed = raw.trim();
        let indent = python_indent(raw);

        if let Some(state) = current.as_ref() {
            if !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && indent <= state.indent
                && !trimmed.starts_with('@')
            {
                finish_python_class(&mut parsed, current.take());
                continue;
            }
        }

        if current.is_none() && indent == 0 {
            if let Some((name, bases)) = python_class_decl(trimmed) {
                if !name.starts_with('_') {
                    let is_exception = python_exception_bases(&bases);
                    current = Some(PythonClassState {
                        name,
                        indent,
                        is_model: model_file && !is_exception,
                        is_exception,
                        model: PythonModelSurface::default(),
                    });
                }
                index += 1;
                continue;
            }
            if model_file {
                if let Some((alias, target)) = python_type_alias(trimmed) {
                    parsed.aliases.insert(alias, target);
                }
            }
        }

        if let Some(state) = current.as_mut() {
            if indent == state.indent + 4 {
                if python_function_start(trimmed).is_some() {
                    let (signature, last) = collect_python_function_signature(&lines, index);
                    if let Some(method) = python_function_start(signature.trim()) {
                        match method.as_str() {
                            "__init__" if state.is_model => {
                                state.model.constructor =
                                    Some(normalize_python_constructor(&signature));
                            }
                            "from_dict" if state.is_model => state.model.has_from_dict = true,
                            "to_dict" if state.is_model => state.model.has_to_dict = true,
                            _ => {}
                        }
                    }
                    index = last + 1;
                    continue;
                }
                if state.is_model {
                    if let Some((field_name, field)) = parse_python_model_field(trimmed) {
                        if let Some(alias) = &field.alias {
                            parsed
                                .aliases
                                .insert(format!("{}.{}", state.name, field_name), alias.clone());
                        }
                        state.model.fields.insert(field_name, field);
                    }
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
    let after = rest[name.len()..].trim_start();
    let bases = if let Some(after) = after.strip_prefix('(') {
        after.split_once(')')?.0.to_string()
    } else {
        String::new()
    };
    line.ends_with(':').then_some((name, bases))
}

fn python_exception_bases(bases: &str) -> bool {
    split_python_top_level(bases, ',').iter().any(|base| {
        let name = base.trim().rsplit('.').next().unwrap_or_default().trim();
        name == "BaseException" || name.ends_with("Exception") || name.ends_with("Error")
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
        let part = lines[index].trim();
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
    let required = default.is_none_or(python_default_is_required);
    let alias = default.and_then(python_field_alias);
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
        let args = args.trim();
        if args.is_empty() || args == "..." || args.starts_with("..., ") {
            return true;
        }
        if args.starts_with("default=")
            || args.contains(", default=")
            || args.starts_with("default_factory=")
            || args.contains(", default_factory=")
        {
            return false;
        }
        let first = split_python_top_level(args, ',')
            .first()
            .map_or("", |value| value.trim());
        return first.contains('=');
    }
    if let Some(args) = python_call_args(default, "field") {
        return !(args.contains("default=") || args.contains("default_factory="));
    }
    false
}

fn python_call_args<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let value = value.strip_prefix(name)?.trim_start();
    let value = value.strip_prefix('(')?;
    value.rsplit_once(')').map(|(args, _)| args)
}

fn python_field_alias(default: &str) -> Option<String> {
    for key in ["alias", "serialization_alias"] {
        if let Some(value) = python_keyword_string(default, key) {
            return Some(value);
        }
    }
    None
}

fn python_keyword_string(value: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let start = value.find(&needle)? + needle.len();
    let rest = value[start..].trim_start();
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let after = &rest[quote.len_utf8()..];
    let end = after.find(quote)?;
    Some(after[..end].to_string())
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
        if parameter.is_empty() || parameter == "self" || parameter == "/" || parameter == "*" {
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
        format!("{normalized} = {}", collapse_python_whitespace(default))
    })
}

fn canonical_python_type(annotation: &str) -> String {
    let mut ty = annotation.trim();
    if ((ty.starts_with('\'') && ty.ends_with('\'')) || (ty.starts_with('"') && ty.ends_with('"')))
        && ty.len() >= 2
    {
        ty = &ty[1..ty.len() - 1];
    }
    let compact = python_compact_type(ty)
        .replace("typing.", "")
        .replace("List[", "list[")
        .replace("Dict[", "dict[")
        .replace("Tuple[", "tuple[")
        .replace("Set[", "set[")
        .replace("NoneType", "None");
    if compact.starts_with("Optional[") && compact.ends_with(']') {
        let inner = &compact[9..compact.len() - 1];
        return canonical_python_union([canonical_python_type(inner), "None".to_string()]);
    }
    if compact.starts_with("Union[") && compact.ends_with(']') {
        let inner = &compact[6..compact.len() - 1];
        return canonical_python_union(
            split_python_top_level(inner, ',')
                .into_iter()
                .map(canonical_python_type),
        );
    }
    let union = split_python_top_level(&compact, '|');
    if union.len() > 1 {
        return canonical_python_union(union.into_iter().map(canonical_python_type));
    }
    compact
}

fn canonical_python_union(parts: impl IntoIterator<Item = String>) -> String {
    let mut parts: Vec<String> = parts.into_iter().collect();
    parts.sort();
    parts.dedup();
    parts.join("|")
}

fn python_type_is_nullable(ty: &str) -> bool {
    split_python_top_level(ty, '|')
        .iter()
        .any(|part| part.trim() == "None")
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
    let left = line[..equal].trim();
    let target = line[equal + 1..].trim();
    let (name, explicit) = left
        .split_once(':')
        .map_or((left, false), |(name, annotation)| {
            (name.trim(), annotation.contains("TypeAlias"))
        });
    if name.starts_with('_') || ident_prefix(name) != Some(name) {
        return None;
    }
    let public_alias = name.chars().next().is_some_and(char::is_uppercase);
    let target_is_type = target
        .trim_start()
        .chars()
        .next()
        .is_some_and(char::is_uppercase);
    if !explicit && (!public_alias || !target_is_type) {
        return None;
    }
    Some((name.to_string(), canonical_python_type(target)))
}

fn parse_python_package_exports(rel: &Path, text: &str) -> Vec<String> {
    let package = rel
        .parent()
        .map(python_path_components)
        .unwrap_or_default()
        .join(".");
    let mut names = parse_python_all(text);
    if names.is_empty() {
        names = parse_python_import_exports(text);
    }
    names
        .into_iter()
        .filter(|name| !name.starts_with('_'))
        .map(|name| {
            if package.is_empty() {
                name
            } else {
                format!("{package}.{name}")
            }
        })
        .collect()
}

fn parse_python_all(text: &str) -> Vec<String> {
    let Some(start) = text.find("__all__") else {
        return Vec::new();
    };
    let Some(equal) = text[start..].find('=') else {
        return Vec::new();
    };
    let value = &text[start + equal + 1..];
    let Some((open, open_char, close_char)) =
        value.char_indices().find_map(|(index, ch)| match ch {
            '[' => Some((index, '[', ']')),
            '(' => Some((index, '(', ')')),
            _ => None,
        })
    else {
        return Vec::new();
    };
    let Some(close) = matching_python_delimiter(value, open, open_char, close_char) else {
        return Vec::new();
    };
    python_quoted_values(&value[open + open_char.len_utf8()..close])
}

fn python_quoted_values(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut quote = None;
    let mut start = 0;
    let mut escape = false;
    for (index, ch) in value.char_indices() {
        if let Some(active) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == active {
                values.push(value[start..index].to_string());
                quote = None;
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
            start = index + ch.len_utf8();
        }
    }
    values.sort();
    values.dedup();
    values
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

fn python_module_name(path: &Path) -> String {
    let mut parts = python_path_components(path);
    if let Some(last) = parts.last_mut() {
        *last = last.trim_end_matches(".py").to_string();
    }
    if parts.last().is_some_and(|part| part == "__init__") {
        parts.pop();
    }
    if parts.is_empty() {
        "__init__".to_string()
    } else {
        parts.join(".")
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
        if path.is_dir() {
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
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("py") {
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
        let text = read_to_string(pyproject)?;
        let mut section = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim_matches('"').to_string();
                continue;
            }
            if section == "project.scripts"
                || section == "project.gui-scripts"
                || section.starts_with("project.entry-points.")
            {
                if let Some((key, value)) = line.split_once('=') {
                    entries.insert(
                        format!("{}.{}", section, key.trim().trim_matches('"')),
                        value
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_string(),
                    );
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
                    if !value.trim().is_empty() {
                        entries.insert(format!("setup.cfg.{group}"), value.trim().to_string());
                    }
                }
            } else if let Some((name, value)) = line.split_once('=') {
                entries.insert(
                    format!("setup.cfg.{group}.{}", name.trim()),
                    value.trim().to_string(),
                );
            }
        }
    }
    Ok(entries)
}

#[derive(Default)]
struct ParsedGoFile {
    types: Vec<String>,
    functions: BTreeMap<String, String>,
    methods: BTreeMap<String, String>,
}

fn parse_go_file(text: &str) -> ParsedGoFile {
    let mut parsed = ParsedGoFile::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        if let Some(name) = go_type_decl(line) {
            if is_go_exported(name) {
                parsed.types.push(name.to_string());
            }
        }
        if let Some((name, signature)) = go_func_decl(line) {
            if is_go_exported(&name) {
                parsed.functions.insert(name, signature);
            }
        }
        if let Some((receiver, method, signature)) = go_method_decl(line) {
            if is_go_exported(&receiver) && is_go_exported(&method) {
                parsed
                    .methods
                    .insert(format!("{receiver}.{method}"), signature);
            }
        }
    }
    parsed.types.sort();
    parsed.types.dedup();
    parsed
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
    let signature = line
        .split_once('{')
        .map_or(line, |(signature, _)| signature)
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
    split_go_top_level_commas(list)
        .into_iter()
        .flat_map(normalize_go_param_decl)
        .collect::<Vec<_>>()
        .join(", ")
}

fn normalize_go_param_decl(decl: &str) -> Vec<String> {
    let decl = collapse_go_signature_whitespace(decl);
    if decl.is_empty() {
        return Vec::new();
    }
    let Some((names, ty)) = split_go_decl_at_top_level_name_space(&decl) else {
        return vec![decl];
    };
    if !is_go_identifier_list(names) {
        return vec![decl];
    }
    let count = names
        .split(',')
        .filter(|name| !name.trim().is_empty())
        .count();
    std::iter::repeat_n(ty.to_string(), count).collect()
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
            if !names.is_empty() && !ty.is_empty() && is_go_identifier_list(names) {
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

fn is_go_identifier_list(value: &str) -> bool {
    value
        .split(',')
        .map(str::trim)
        .all(|part| !part.is_empty() && ident_prefix(part) == Some(part))
}

fn collapse_go_signature_whitespace(signature: &str) -> String {
    signature.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_go_exported(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
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
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("vendor") {
                continue;
            }
            collect_go_files(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("go")
            && path.file_name().and_then(|name| name.to_str()) != Some("go.mod")
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
        if path.is_dir() {
            if matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("vendor" | "node_modules")
            ) {
                continue;
            }
            collect_doc_files(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
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
