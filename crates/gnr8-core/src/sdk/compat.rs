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
            || !self.package_entry_point_changes.is_empty()
    }
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

    for rel in &files {
        let text = read_to_string(dir.join(rel))?;
        let parsed = parse_ts_file(&text);
        merge_exports(&mut all_exports, &parsed.exports);
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
        package_entry_points: package_entry_points(dir)?,
    })
}

/// Diff two already-extracted TypeScript surfaces.
#[must_use]
pub fn diff_typescript_surfaces(
    old: &TypeScriptSurface,
    new: &TypeScriptSurface,
) -> TypeScriptSurfaceDiff {
    TypeScriptSurfaceDiff {
        missing_root_exports: missing_keys(&old.root_exports, &new.root_exports),
        missing_model_exports: missing_keys(&old.model_exports, &new.model_exports),
        export_kind_mismatches: kind_mismatches(&old.root_exports, &new.root_exports),
        missing_api_classes: missing_values(&old.api_classes, &new.api_classes),
        missing_api_factories: missing_values(&old.api_factories, &new.api_factories),
        missing_operation_methods: missing_values(&old.operation_methods, &new.operation_methods),
        missing_request_aliases: missing_values(&old.request_aliases, &new.request_aliases),
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
}

fn parse_ts_file(text: &str) -> ParsedTsFile {
    let mut parsed = ParsedTsFile::default();
    let mut current_api_class: Option<(String, i32)> = None;
    for raw in text.lines() {
        let line = raw.trim();
        let mut starts_api_class = false;
        if let Some(name) = strip_export_decl(line, "interface") {
            add_export(&mut parsed.exports, name, TsExportKind::Type);
            if name.ends_with("Request") {
                parsed.request_aliases.push(name.to_string());
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
            }
        } else if let Some(exports) = line.strip_prefix("export type {") {
            parse_export_list(exports, TsExportKind::Type, &mut parsed.exports);
        } else if let Some(exports) = line.strip_prefix("export {") {
            parse_export_list(exports, TsExportKind::Value, &mut parsed.exports);
        }

        let mut close_api_class = false;
        if let Some((class_name, depth)) = &mut current_api_class {
            if !starts_api_class {
                if let Some(method) = strip_async_method(line) {
                    parsed
                        .operation_methods
                        .push(format!("{class_name}.{method}"));
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
    }
    parsed
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

fn strip_async_method(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("async ")?;
    ident_prefix(rest)
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

fn missing_keys(
    old: &BTreeMap<String, TsExportKind>,
    new: &BTreeMap<String, TsExportKind>,
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{diff_typescript_dirs, extract_typescript_surface, TsExportKind};

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
            "export interface Book {}\nexport const Format = {} as const;\nexport type Format = typeof Format[keyof typeof Format];\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("api.ts"),
            "export interface CreateBookRequest {}\nexport class DefaultApi {\n  async createBook(): Promise<void> {\n    if (true) {\n      return;\n    }\n  }\n  async listBooks(): Promise<void> { return; }\n}\nexport const DefaultApiFactory = function () {};\n",
        )
        .unwrap();

        let surface = extract_typescript_surface(&dir).unwrap();
        assert_eq!(surface.root_exports["Book"], TsExportKind::Type);
        assert_eq!(surface.root_exports["Format"], TsExportKind::Both);
        assert_eq!(surface.api_classes, vec!["DefaultApi"]);
        assert_eq!(surface.api_factories, vec!["DefaultApiFactory"]);
        assert_eq!(
            surface.operation_methods,
            vec!["DefaultApi.createBook", "DefaultApi.listBooks"]
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
            "export interface Book {}\nexport const Format = {} as const;\nexport type Format = typeof Format[keyof typeof Format];\n",
        )
        .unwrap();
        std::fs::write(old.join("api.ts"), "export interface CreateBookRequest {}\nexport class DefaultApi {\n  async createBook(): Promise<void> { return; }\n}\nexport const DefaultApiFactory = function () {};\n").unwrap();
        std::fs::write(
            new.join("models.ts"),
            "export interface Book {}\nexport type Format = \"hardcover\";\n",
        )
        .unwrap();
        std::fs::write(new.join("api.ts"), "export class DefaultApi {}\n").unwrap();

        let diff = diff_typescript_dirs(&old, &new).unwrap();
        assert!(diff.is_breaking());
        assert_eq!(diff.export_kind_mismatches[0].symbol, "Format");
        assert_eq!(diff.missing_api_factories, vec!["DefaultApiFactory"]);
        assert_eq!(
            diff.missing_operation_methods,
            vec!["DefaultApi.createBook"]
        );
        assert_eq!(diff.missing_request_aliases, vec!["CreateBookRequest"]);

        let _ = std::fs::remove_dir_all(old);
        let _ = std::fs::remove_dir_all(new);
    }
}
