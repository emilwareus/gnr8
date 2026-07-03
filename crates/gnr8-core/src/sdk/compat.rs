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
    }
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
        docs: go_doc_files(dir)?,
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
    }
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

fn go_doc_files(dir: &Path) -> Result<Vec<String>, CoreError> {
    let mut out = Vec::new();
    collect_doc_files(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_doc_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), CoreError> {
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
            collect_doc_files(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            let rel = path
                .strip_prefix(root)
                .map_err(|err| CoreError::Workspace {
                    message: format!("failed to relativize Go doc file {}: {err}", path.display()),
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

    use super::{diff_go_dirs, diff_typescript_dirs, extract_typescript_surface, TsExportKind};

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
}
