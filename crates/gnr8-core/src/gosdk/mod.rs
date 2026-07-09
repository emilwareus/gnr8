//! Go SDK generation seam (Phase 3): generates a Go SDK from the API graph.
//!
//! [`generate`] turns the Phase-2 [`crate::graph::ApiGraph`] into a single deterministic, `gofmt`-clean
//! Go SDK bundle String (D-06): one functional-options `client.go`, one typed `errors.go`, one generic
//! `operations.go` resource surface, and one `models.go`. Tags were an annotation fact and have been
//! removed (CLAUDE.md rules 1 & 3), so the SDK is a single operations surface rather than per-tag files.
//! The package name is supplied by the caller (derived from the `GoSdk` target's module path, the
//! single source of truth — see [`crate::sdk::builtins::GoSdk`]). Each file is emitted by [`emit`]
//! (`format!`-based, no template engine — D-05), normalized through the real `gofmt` ([`gofmt`]), and
//! framed into an [`bundle::SdkBundle`] with stable file markers. The pipeline is byte-identical across
//! runs and never panics (RUST-04); [`write_to_dir`] materializes the same framing for 03-03's compile
//! test.

mod emit;
mod gofmt;

use std::collections::{BTreeMap, BTreeSet};

use crate::graph::{ApiGraph, Operation};
use crate::sdk::bundle::{check_unique_file_names, SdkBundle, SdkFile};
use crate::sdk::emit_common::{
    api_key_credential_names, check_unique_schema_names, file_stem, model_file_name,
    operation_file_name, operation_group_file_name, operation_group_name, validate_sdk_base_path,
};
use crate::sdk::go::GoSdkOptions;
use crate::sdk::go::{GoQuerySetterArgumentPolicy, GoRequestBuilderAliases};
use crate::sdk::layout::{OperationFileSplit, SdkFileLayout};
use crate::sdk::profile::SdkProfile;
use crate::sdk::surface::SdkTypeAliases;

/// Generate the Go SDK as a deterministic, `gofmt`-clean multi-file bundle String (D-06, SDK-01..04).
///
/// Emits `client.go` (functional-options `Client`), `errors.go` (typed `APIError`), one generic
/// `operations.go` (`context.Context`-first methods on `*Client`), and `models.go` (request/response
/// structs + enum newtypes), pipes each through `gofmt`, and frames them into a single
/// [`bundle::SdkBundle`] String. Generating twice over the same graph is byte-identical (T-03-02-03).
///
/// `package` is the SDK's Go package name — derived from the `GoSdk` target's module path (the single
/// source of truth) via [`crate::sdk::builtins::GoSdk`]; it appears in every file's `package` clause.
/// `base_path` is the API base/mount path joined to each operation's group-relative path in the emitted
/// request URLs — the SAME single source of truth (the graph's `base_path`, set by a `SetBasePath`
/// transform) the `OpenAPI` lowering takes it from (CLAUDE.md rules 3 & 4), so the SDK and the spec
/// agree on the prefix.
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] for an un-representable graph fact (dangling `$ref`, unknown
/// `kind`), [`crate::CoreError::GoFmt`] if `gofmt` rejects emitted Go, or
/// [`crate::CoreError::GoToolchainMissing`] if `gofmt` cannot be spawned.
pub fn generate(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
) -> Result<String, crate::CoreError> {
    generate_with_layout(graph, package, base_path, &SdkFileLayout::compact())
}

/// Generate the Go SDK with a configurable file layout.
///
/// # Errors
///
/// Returns the same errors as [`generate`].
pub fn generate_with_layout(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
) -> Result<String, crate::CoreError> {
    let aliases = SdkTypeAliases::default();
    let files = generate_files_with_layout(graph, package, base_path, layout, &aliases)?;
    let bundle = SdkBundle { files };
    Ok(bundle.to_string())
}

pub(crate) fn generate_files_with_layout(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    generate_files_with_profile(
        graph,
        package,
        base_path,
        layout,
        aliases,
        &SdkProfile::minimal(),
    )
}

pub(crate) fn generate_files_with_profile(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    profile: &SdkProfile,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    generate_files_with_profile_options(
        graph,
        package,
        base_path,
        layout,
        aliases,
        profile,
        GoSdkOptions::for_profile(profile),
    )
}

pub(crate) fn generate_files_with_profile_options(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    profile: &SdkProfile,
    options: GoSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    validate_sdk_base_path(base_path)?;
    check_unique_schema_names(graph, "Go SDK")?;

    if profile.is_go_openapi_generator_compat() {
        return generate_go_openapi_generator_compat_files(
            graph, package, base_path, aliases, options,
        );
    }

    let mut files: Vec<SdkFile> = Vec::new();
    let auth_credentials = api_key_credential_names(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;
    let emit_compat_surface =
        profile.is_go_openapi_generator_compat() || aliases.has_source_prefix_aliases();
    let compat_options = emit::GoEmitOptions {
        compat_model_helpers: emit_compat_surface,
        sdk: options.clone(),
    };

    // Fixed leading files (sorted: client.go before errors.go).
    files.push(raw_go_file(
        "client.go",
        emit::emit_client(package, !auth_credentials.is_empty()),
    ));
    files.push(raw_go_file("errors.go", emit::emit_errors(package)));
    if emit_compat_surface {
        files.push(raw_go_file(
            "compat_helpers.go",
            emit::emit_compat_helpers(package),
        ));
        files.push(raw_go_file(
            "compat_client.go",
            emit::emit_compat_client_surface(graph, package, base_path)?,
        ));
    }
    if !resolved_aliases.is_empty() {
        files.push(raw_go_file(
            "aliases.go",
            emit::emit_type_aliases(graph, package, &resolved_aliases, &compat_options)?,
        ));
    }
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    if layout.is_split() {
        match layout.operation_split() {
            OperationFileSplit::Compact => {
                files.push(raw_go_file(
                    "operations.go",
                    emit::emit_operations_without_facades(graph, package, base_path, &ops)?,
                ));
            }
            OperationFileSplit::PerEndpoint => {
                for op in &ops {
                    let raw =
                        emit::emit_operations_without_facades(graph, package, base_path, &[*op])?;
                    let name =
                        operation_file_name(layout, op, &format!("api_{}.go", file_stem(&op.id)))?;
                    files.push(raw_go_file(name, raw));
                }
            }
            OperationFileSplit::PerTag => {
                for (group, group_ops) in operation_groups(&ops) {
                    let raw = emit::emit_operations_without_facades(
                        graph, package, base_path, &group_ops,
                    )?;
                    let name = operation_group_file_name(
                        layout,
                        &group,
                        &format!("api_{}.go", file_stem(&group)),
                    )?;
                    files.push(raw_go_file(name, raw));
                }
            }
        }
        if let Some(raw) = emit::emit_facades(graph, package, &ops)? {
            files.push(raw_go_file("facades.go", raw));
        }
        for schema in &graph.schemas {
            let raw =
                emit::emit_model_schema_with_options(graph, package, schema, &compat_options)?;
            let name = model_file_name(
                layout,
                schema,
                &format!("model_{}.go", file_stem(&schema.name)),
            )?;
            files.push(raw_go_file(name, raw));
        }
    } else {
        // All operations go into a single generic `operations.go` resource surface. Tags were an
        // annotation fact and have been removed (CLAUDE.md rules 1 & 3), so there is no per-tag grouping;
        // the file name is generic (not the package/fixture name) so it never overfits to one service.
        let raw = emit::emit_operations(graph, package, base_path, &ops)?;
        files.push(raw_go_file("operations.go", raw));

        // Trailing models.go.
        files.push(raw_go_file(
            "models.go",
            emit::emit_models_with_options(graph, package, &compat_options)?,
        ));
    }

    check_unique_file_names(&files, "Go SDK")?;
    let mut files = gofmt::gofmt_files(files)?;
    check_unique_file_names(&files, "Go SDK")?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn operation_groups<'op>(ops: &[&'op Operation]) -> BTreeMap<String, Vec<&'op Operation>> {
    let mut groups: BTreeMap<String, Vec<&Operation>> = BTreeMap::new();
    for op in ops {
        groups
            .entry(operation_group_name(op).to_string())
            .or_default()
            .push(*op);
    }
    groups
}

fn generate_go_openapi_generator_compat_files(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    aliases: &SdkTypeAliases,
    options: GoSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    let options = resolve_go_compat_options(graph, options)?;
    if let Some(error_model) = &options.error_model {
        validate_go_error_model_name(error_model)?;
    }
    validate_go_compat_options(graph, &options)?;
    let resolved_aliases = aliases.resolve(graph)?;
    let compat_options = emit::GoEmitOptions {
        compat_model_helpers: true,
        sdk: options,
    };
    let mut files = vec![
        raw_go_file(
            "client.go",
            emit::emit_compat_api_client_file(graph, package)?,
        ),
        raw_go_file("configuration.go", emit::emit_compat_configuration(package)),
        raw_go_file(
            "errors.go",
            emit::emit_compat_errors(package, &compat_options),
        ),
        raw_go_file(
            "utils.go",
            emit::emit_compat_utils(package, &compat_options),
        ),
    ];
    for (service, ops) in emit::compat_operations_by_service(graph) {
        files.push(raw_go_file(
            format!("api_{}.go", file_stem(&service)),
            emit::emit_compat_api_file(graph, package, base_path, &service, &ops, &compat_options)?,
        ));
    }
    if !resolved_aliases.is_empty() {
        files.push(raw_go_file(
            "aliases.go",
            emit::emit_type_aliases(graph, package, &resolved_aliases, &compat_options)?,
        ));
    }
    for schema in &graph.schemas {
        files.push(raw_go_file(
            format!("model_{}.go", file_stem(&schema.name)),
            emit::emit_model_schema_with_options(graph, package, schema, &compat_options)?,
        ));
    }
    check_unique_file_names(&files, "Go SDK")?;
    let mut files = gofmt::gofmt_files(files)?;
    check_unique_file_names(&files, "Go SDK")?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn resolve_go_compat_options(
    graph: &ApiGraph,
    mut options: GoSdkOptions,
) -> Result<GoSdkOptions, crate::CoreError> {
    options.request_builder_aliases =
        resolve_request_builder_aliases(graph, &options.request_builder_aliases)?;
    options.query_setter_argument_policy =
        resolve_query_setter_argument_policy(graph, &options.query_setter_argument_policy)?;
    options.execute_compatibility =
        resolve_execute_compatibility(graph, &options.execute_compatibility)?;
    Ok(options)
}

fn resolve_request_builder_aliases(
    graph: &ApiGraph,
    aliases: &GoRequestBuilderAliases,
) -> Result<GoRequestBuilderAliases, crate::CoreError> {
    let mut resolved = GoRequestBuilderAliases::new();
    for (selector, setters) in &aliases.body {
        let request = resolve_request_selector(graph, selector)?;
        for setter in setters {
            resolved = resolved.body(request.clone(), setter.clone());
        }
    }
    for (selector, query_aliases) in &aliases.query {
        let request = resolve_request_selector(graph, selector)?;
        for alias in query_aliases {
            resolved = resolved.query(
                request.clone(),
                alias.setter.clone(),
                alias.query_name.clone(),
            );
        }
    }
    Ok(resolved)
}

fn resolve_query_setter_argument_policy(
    graph: &ApiGraph,
    policy: &GoQuerySetterArgumentPolicy,
) -> Result<GoQuerySetterArgumentPolicy, crate::CoreError> {
    let GoQuerySetterArgumentPolicy::SelectiveAny(any_for) = policy else {
        return Ok(policy.clone());
    };
    let mut resolved = GoQuerySetterArgumentPolicy::typed();
    for (selector, setters) in any_for {
        if selector == "operation:*" {
            for setter in setters {
                let Some(query_name) = setter.strip_prefix("query:") else {
                    return Err(crate::CoreError::Config {
                        message: format!(
                            "GoSdk query_setter_argument_policy global selector requires query:name, got '{setter}'"
                        ),
                    });
                };
                resolved = widen_query_name_on_matching_operations(graph, resolved, query_name)?;
            }
            continue;
        }

        let request = resolve_request_selector(graph, selector)?;
        for setter in setters {
            if let Some(query_name) = setter.strip_prefix("query:") {
                resolved = widen_query_name_on_request(graph, resolved, &request, query_name)?;
            } else {
                resolved = resolved.any_for(request.clone(), setter.clone());
            }
        }
    }
    Ok(resolved)
}

fn resolve_execute_compatibility(
    graph: &ApiGraph,
    compatibility: &crate::sdk::go::GoExecuteCompatibility,
) -> Result<crate::sdk::go::GoExecuteCompatibility, crate::CoreError> {
    let mut resolved = crate::sdk::go::GoExecuteCompatibility::preserve_legacy();
    for request in compatibility.request_names() {
        resolved = resolved.request(request.clone());
    }
    for operation in compatibility.operation_names() {
        if operation.starts_with("route:") {
            let op = resolve_operation_selector(graph, operation)?;
            resolved = resolved.operation(op.id.clone());
        } else {
            resolved = resolved.operation(operation.clone());
        }
    }
    Ok(resolved)
}

fn widen_query_name_on_matching_operations(
    graph: &ApiGraph,
    mut policy: GoQuerySetterArgumentPolicy,
    query_name: &str,
) -> Result<GoQuerySetterArgumentPolicy, crate::CoreError> {
    let mut matched = false;
    for op in &graph.operations {
        if op
            .params
            .iter()
            .any(|param| param.location == "query" && param.name == query_name)
        {
            matched = true;
            let request = emit::compat_request_name(op);
            policy = widen_query_name_on_request(graph, policy, &request, query_name)?;
        }
    }
    if matched {
        Ok(policy)
    } else {
        Err(crate::CoreError::Config {
            message: format!(
                "GoSdk query_setter_argument_policy query selector did not match any query parameter '{query_name}'"
            ),
        })
    }
}

fn widen_query_name_on_request(
    graph: &ApiGraph,
    mut policy: GoQuerySetterArgumentPolicy,
    request: &str,
    query_name: &str,
) -> Result<GoQuerySetterArgumentPolicy, crate::CoreError> {
    let op = graph
        .operations
        .iter()
        .find(|op| emit::compat_request_name(op) == request)
        .ok_or_else(|| crate::CoreError::Config {
            message: format!("GoSdk query selector references unknown request builder '{request}'"),
        })?;
    let param = op
        .params
        .iter()
        .find(|param| param.location == "query" && param.name == query_name)
        .ok_or_else(|| crate::CoreError::Config {
            message: format!(
                "GoSdk query selector references unknown query parameter '{query_name}' on '{request}'"
            ),
        })?;
    for setter in emit::compat_method_names(&emit::compat_exported(&param.name)) {
        policy = policy.any_for(request.to_string(), setter);
    }
    Ok(policy)
}

fn resolve_request_selector(graph: &ApiGraph, selector: &str) -> Result<String, crate::CoreError> {
    if selector.starts_with("operation:") || selector.starts_with("route:") {
        return resolve_operation_selector(graph, selector).map(emit::compat_request_name);
    }
    Ok(selector.to_string())
}

fn resolve_operation_selector<'a>(
    graph: &'a ApiGraph,
    selector: &str,
) -> Result<&'a Operation, crate::CoreError> {
    if let Some(operation_id) = selector.strip_prefix("operation:") {
        let matches: Vec<&Operation> = graph
            .operations
            .iter()
            .filter(|op| op.id == operation_id)
            .collect();
        return single_operation_match(selector, &matches);
    }
    if let Some(route) = selector.strip_prefix("route:") {
        let Some((method, path)) = route.split_once(' ') else {
            return Err(crate::CoreError::Config {
                message: format!("invalid GoSdk operation selector '{selector}'"),
            });
        };
        let matches: Vec<&Operation> = graph
            .operations
            .iter()
            .filter(|op| op.method == method && op.path == path)
            .collect();
        return single_operation_match(selector, &matches);
    }
    Err(crate::CoreError::Config {
        message: format!("invalid GoSdk operation selector '{selector}'"),
    })
}

fn single_operation_match<'a>(
    selector: &str,
    matches: &[&'a Operation],
) -> Result<&'a Operation, crate::CoreError> {
    match matches {
        [single] => Ok(*single),
        [] => Err(crate::CoreError::Config {
            message: format!("GoSdk operation selector did not match any operation: {selector}"),
        }),
        many => Err(crate::CoreError::Config {
            message: format!(
                "GoSdk operation selector matched {} operations: {selector}",
                many.len()
            ),
        }),
    }
}

fn validate_go_compat_options(
    graph: &ApiGraph,
    options: &GoSdkOptions,
) -> Result<(), crate::CoreError> {
    let request_names: BTreeSet<String> = graph
        .operations
        .iter()
        .map(emit::compat_request_name)
        .collect();
    let operation_ids: BTreeSet<&str> = graph.operations.iter().map(|op| op.id.as_str()).collect();

    for request in options.request_builder_aliases.request_names() {
        if !request_names.contains(&request) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk request_builder_aliases references unknown request builder '{request}'"
                ),
            });
        }
    }
    for (request, aliases) in &options.request_builder_aliases.body {
        for alias in aliases {
            validate_go_exported_method_name(alias, "request_builder_aliases body setter")?;
        }
        if !request_names.contains(request) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk request_builder_aliases references unknown request builder '{request}'"
                ),
            });
        }
    }
    for (request, aliases) in &options.request_builder_aliases.query {
        for alias in aliases {
            validate_go_exported_method_name(
                &alias.setter,
                "request_builder_aliases query setter",
            )?;
        }
        if !request_names.contains(request) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk request_builder_aliases references unknown request builder '{request}'"
                ),
            });
        }
    }
    validate_go_request_builder_alias_conflicts(options)?;
    validate_go_query_setter_argument_policy(graph, options, &request_names)?;
    for request in options.execute_compatibility.request_names() {
        if !request_names.contains(request) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk execute_compatibility references unknown request builder '{request}'"
                ),
            });
        }
    }
    for operation in options.execute_compatibility.operation_names() {
        if !operation_ids.contains(operation.as_str()) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk execute_compatibility references unknown operation '{operation}'"
                ),
            });
        }
    }
    Ok(())
}

fn validate_go_query_setter_argument_policy(
    graph: &ApiGraph,
    options: &GoSdkOptions,
    request_names: &BTreeSet<String>,
) -> Result<(), crate::CoreError> {
    for request in options.query_setter_argument_policy.request_names() {
        if !request_names.contains(&request) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk query_setter_argument_policy references unknown request builder '{request}'"
                ),
            });
        }
        let valid_setters = go_query_setter_methods_for_request(graph, &request);
        for setter in options.query_setter_argument_policy.setters_for(&request) {
            if !valid_setters.contains(&setter) {
                return Err(crate::CoreError::Config {
                    message: format!(
                        "GoSdk query_setter_argument_policy references unknown query setter '{setter}' on '{request}'"
                    ),
                });
            }
        }
    }
    Ok(())
}

fn go_query_setter_methods_for_request(graph: &ApiGraph, request: &str) -> BTreeSet<String> {
    graph
        .operations
        .iter()
        .find(|op| emit::compat_request_name(op) == request)
        .map(|op| {
            op.params
                .iter()
                .filter(|param| param.location == "query")
                .flat_map(|param| emit::compat_method_names(&emit::compat_exported(&param.name)))
                .collect()
        })
        .unwrap_or_default()
}

fn validate_go_request_builder_alias_conflicts(
    options: &GoSdkOptions,
) -> Result<(), crate::CoreError> {
    for request in options.request_builder_aliases.request_names() {
        let mut methods = BTreeSet::new();
        for alias in options.request_builder_aliases.body_aliases_for(&request) {
            validate_go_request_builder_alias_method(&request, &alias, &mut methods)?;
        }
        for alias in options.request_builder_aliases.query_aliases_for(&request) {
            validate_go_request_builder_alias_method(&request, &alias.setter, &mut methods)?;
        }
    }
    Ok(())
}

fn validate_go_request_builder_alias_method(
    request: &str,
    alias: &str,
    methods: &mut BTreeSet<String>,
) -> Result<(), crate::CoreError> {
    for method in emit::compat_method_names(alias) {
        if emit::compat_request_reserved_method(&method) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk request_builder_aliases for '{request}' conflicts with reserved method '{method}'"
                ),
            });
        }
        if !methods.insert(method.clone()) {
            return Err(crate::CoreError::Config {
                message: format!(
                    "GoSdk request_builder_aliases for '{request}' contains conflicting method '{method}'"
                ),
            });
        }
    }
    Ok(())
}

/// Wrap a raw emitted file as a named [`SdkFile`] before batched formatting.
fn raw_go_file(name: impl Into<String>, raw: impl Into<String>) -> SdkFile {
    SdkFile {
        name: name.into(),
        contents: raw.into(),
    }
}

fn validate_go_exported_method_name(name: &str, field: &str) -> Result<(), crate::CoreError> {
    if is_go_identifier(name)
        && !is_go_keyword(name)
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        return Ok(());
    }
    Err(crate::CoreError::Config {
        message: format!(
            "GoSdk {field} must be a valid exported Go method identifier, got '{name}'"
        ),
    })
}

fn validate_go_error_model_name(name: &str) -> Result<(), crate::CoreError> {
    if is_go_identifier(name) && !is_go_keyword(name) {
        return Ok(());
    }
    Err(crate::CoreError::Config {
        message: format!("GoSdk error_model must be a valid Go type identifier, got '{name}'"),
    })
}

fn is_go_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_go_keyword(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "default"
            | "func"
            | "interface"
            | "select"
            | "case"
            | "defer"
            | "go"
            | "map"
            | "struct"
            | "chan"
            | "else"
            | "goto"
            | "package"
            | "switch"
            | "const"
            | "fallthrough"
            | "if"
            | "range"
            | "type"
            | "continue"
            | "for"
            | "import"
            | "return"
            | "var"
    )
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code. These tests require the Go
    // toolchain (generate runs gofmt) and skip gracefully if it is absent.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        generate, generate_files_with_layout, generate_files_with_profile,
        generate_files_with_profile_options, generate_with_layout,
    };
    use crate::analyze::facts::FieldMeta;
    use crate::graph::{
        ApiGraph, Field, Operation, Param, Prim, Response, Schema, SchemaRef, SecurityScheme,
        SourceSpan, Type, WellKnown,
    };
    use crate::sdk::go::{
        GoExecuteCompatibility, GoQuerySetterArgumentPolicy, GoRequestBuilderAliases, GoSdkOptions,
        QueryTimeFormat, RequiredPointerConstructorPolicy,
    };
    use crate::sdk::layout::SdkFileLayout;
    use crate::sdk::profile::SdkProfile;
    use crate::sdk::surface::SdkTypeAliases;

    /// A facts document (code-first shape — no annotation facts) covering three operations plus the
    /// fixture request/response models + the code-defined `TargetDirection` enum — enough to assert the
    /// bundle shape without the live fixture. Mirrors the real graph's relevant subset.
    const SAMPLE: &[u8] = br#"{
      "module": "github.com/acme/svc",
      "routes": [
        {
          "method": "POST", "path": "/", "handler": "createGoal",
          "operation_id": "createGoal", "params": [],
          "request_body": { "ref_id": "dto.CreateGoalInput" },
          "responses": [
            { "status": 201, "body": { "ref_id": "dto.CommandMessage" } },
            { "status": 400, "body": { "ref_id": "dto.HttpError" } }
          ],
          "span": { "file": "/root/http.go", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "PUT", "path": "/{uuid}", "handler": "updateGoal",
          "operation_id": "updateGoal",
          "params": [
            { "name": "uuid", "location": "path", "required": true,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/h.go", "start_line": 1, "end_line": 1 } }
          ],
          "request_body": { "ref_id": "dto.UpdateGoalInput" },
          "responses": [ { "status": 200, "body": { "ref_id": "dto.CommandMessage" } } ],
          "span": { "file": "/root/http.go", "start_line": 2, "end_line": 2 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listGoals",
          "operation_id": "listGoals",
          "params": [
            { "name": "aggregation", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/h.go", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "dto.ListGoalsOutput" } } ],
          "span": { "file": "/root/http.go", "start_line": 3, "end_line": 3 }
        }
      ],
      "schemas": [
        {
          "id": "dto.CommandMessage", "name": "CommandMessage",
          "body": { "type": "object", "of": [
            { "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/c.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.CreateGoalInput", "name": "CreateGoalInput",
          "body": { "type": "object", "of": [
            { "json_name": "name", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null },
            { "json_name": "targetDirection", "required": false, "optional": true, "nullable": true,
              "schema": { "type": "named", "of": "dto.TargetDirection" },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "dto.HttpError", "name": "HttpError",
          "body": { "type": "object", "of": [
            { "json_name": "message", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/c.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.ListGoalsOutput", "name": "ListGoalsOutput",
          "body": { "type": "object", "of": [
            { "json_name": "total", "required": false, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "dto.TargetDirection", "name": "TargetDirection",
          "body": { "type": "enum", "of": ["gte","lte"] },
          "span": { "file": "/root/c.go", "start_line": 3, "end_line": 3 }
        },
        {
          "id": "dto.UpdateGoalInput", "name": "UpdateGoalInput",
          "body": { "type": "object", "of": [
            { "json_name": "name", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/g.go", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    /// Whether `gofmt` is available so toolchain-dependent tests skip gracefully.
    fn gofmt_available() -> bool {
        std::process::Command::new("gofmt")
            .arg("-h")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    }

    #[test]
    fn generate_returns_ok_with_the_four_file_markers() {
        if !gofmt_available() {
            eprintln!("skipping generate test: gofmt unavailable");
            return;
        }
        let out = generate(&sample_graph(), "goalservice", "/goal").unwrap();
        for marker in [
            "// ==== gnr8:file client.go ====",
            "// ==== gnr8:file errors.go ====",
            "// ==== gnr8:file operations.go ====",
            "// ==== gnr8:file models.go ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
    }

    #[test]
    fn generate_is_byte_identical_across_two_runs() {
        if !gofmt_available() {
            eprintln!("skipping determinism test: gofmt unavailable");
            return;
        }
        let graph = sample_graph();
        assert_eq!(
            generate(&graph, "goalservice", "/goal").unwrap(),
            generate(&graph, "goalservice", "/goal").unwrap(),
            "two generate runs must be byte-identical"
        );
    }

    #[test]
    fn generated_models_contain_the_request_response_models_and_enum() {
        if !gofmt_available() {
            eprintln!("skipping models test: gofmt unavailable");
            return;
        }
        let out = generate(&sample_graph(), "goalservice", "/goal").unwrap();
        for ty in [
            "type CreateGoalInput struct",
            "type UpdateGoalInput struct",
            "type ListGoalsOutput struct",
            "type TargetDirection string",
        ] {
            assert!(out.contains(ty), "missing {ty}:\n{out}");
        }
    }

    #[test]
    fn generated_goals_file_has_ctx_first_create_goal_method() {
        if !gofmt_available() {
            eprintln!("skipping ops test: gofmt unavailable");
            return;
        }
        let out = generate(&sample_graph(), "goalservice", "/goal").unwrap();
        assert!(
            out.contains("func (c *Client) CreateGoal(ctx context.Context"),
            "CreateGoal must take ctx first:\n{out}"
        );
    }

    #[test]
    fn split_layout_defaults_to_one_operation_file_per_tag() {
        if !gofmt_available() {
            eprintln!("skipping split layout test: gofmt unavailable");
            return;
        }
        let out = generate_with_layout(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
        )
        .unwrap();
        for marker in [
            "// ==== gnr8:file api_default.go ====",
            "// ==== gnr8:file model_create_goal_input.go ====",
            "// ==== gnr8:file model_target_direction.go ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
        assert!(
            !out.contains("// ==== gnr8:file operations.go ===="),
            "split layout must not emit the compact operations file:\n{out}"
        );
        assert!(
            !out.contains("// ==== gnr8:file models.go ===="),
            "split layout must not emit the compact models file:\n{out}"
        );
    }

    #[test]
    fn split_layout_can_emit_one_operation_file_per_endpoint() {
        if !gofmt_available() {
            eprintln!("skipping split endpoint layout test: gofmt unavailable");
            return;
        }
        let layout = SdkFileLayout::split().operations_per_endpoint();
        let out = generate_with_layout(&sample_graph(), "goalservice", "/goal", &layout).unwrap();
        for marker in [
            "// ==== gnr8:file api_create_goal.go ====",
            "// ==== gnr8:file api_list_goals.go ====",
            "// ==== gnr8:file api_update_goal.go ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
    }

    #[test]
    fn split_operation_template_rejects_duplicate_rendered_files() {
        if !gofmt_available() {
            eprintln!("skipping split duplicate layout test: gofmt unavailable");
            return;
        }
        let layout = SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_file_template("api_{service_snake}.go");
        let err =
            generate_with_layout(&sample_graph(), "goalservice", "/goal", &layout).unwrap_err();
        assert!(
            err.to_string().contains("duplicate SDK file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn split_layout_emits_group_facades_once() {
        if !gofmt_available() {
            eprintln!("skipping split facade test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Goals".to_string());
        }
        let out =
            generate_with_layout(&graph, "goalservice", "/goal", &SdkFileLayout::split()).unwrap();
        assert!(
            out.contains("// ==== gnr8:file facades.go ===="),
            "split layout should emit a dedicated facade file:\n{out}"
        );
        assert_eq!(out.matches("type GoalsAPI struct").count(), 1, "{out}");
        assert_eq!(
            out.matches("func (c *Client) Goals() *GoalsAPI").count(),
            1,
            "{out}"
        );
    }

    #[test]
    fn split_layout_with_compact_operations_emits_group_facades_once() {
        if !gofmt_available() {
            eprintln!("skipping split compact operations facade test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Goals".to_string());
        }
        let layout = SdkFileLayout::split().compact_operations();
        let out = generate_with_layout(&graph, "goalservice", "/goal", &layout).unwrap();
        assert!(
            out.contains("// ==== gnr8:file operations.go ===="),
            "compact operations should emit operations.go:\n{out}"
        );
        assert!(
            !out.contains("// ==== gnr8:file api_goals.go ===="),
            "compact operations should not emit split operation files:\n{out}"
        );
        assert!(
            out.contains("// ==== gnr8:file facades.go ===="),
            "split layout should keep facades in a dedicated file:\n{out}"
        );
        assert_eq!(out.matches("type GoalsAPI struct").count(), 1, "{out}");
        assert_eq!(
            out.matches("func (c *Client) Goals() *GoalsAPI").count(),
            1,
            "{out}"
        );
    }

    #[test]
    fn split_layout_can_place_operation_and_model_files_in_configured_dirs() {
        if !gofmt_available() {
            eprintln!("skipping custom split layout test: gofmt unavailable");
            return;
        }
        let layout = SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_dir("apis")
            .model_dir("types");
        let out = generate_with_layout(&sample_graph(), "goalservice", "/goal", &layout).unwrap();
        for marker in [
            "// ==== gnr8:file apis/api_create_goal.go ====",
            "// ==== gnr8:file types/model_create_goal_input.go ====",
        ] {
            assert!(out.contains(marker), "missing {marker}:\n{out}");
        }
    }

    #[test]
    fn source_prefix_aliases_emit_grouped_go_compat_client_surface() {
        if !gofmt_available() {
            eprintln!("skipping compat client test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Goals".to_string());
        }
        let aliases = SdkTypeAliases::new().source_prefix_alias("dto.", "Dto");
        let files = generate_files_with_layout(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &aliases,
        )
        .unwrap();
        let compat = files
            .iter()
            .find(|file| file.name == "compat_client.go")
            .map(|file| file.contents.as_str())
            .expect("compat_client.go should be emitted");

        for snippet in [
            "func NewConfiguration() *Configuration",
            "func NewAPIClient(cfg *Configuration) *APIClient",
            "GoalsAPI   *GoalsAPIService",
            "func (a *GoalsAPIService) ListGoals(ctx context.Context) ApiListGoalsRequest",
            "func (r ApiListGoalsRequest) Aggregation(aggregation string) ApiListGoalsRequest",
            "func (r ApiCreateGoalRequest) GoalInput(goalInput any) ApiCreateGoalRequest",
            "func (r ApiListGoalsRequest) Execute() (*ListGoalsOutput, *http.Response, error)",
        ] {
            assert!(compat.contains(snippet), "missing {snippet}:\n{compat}");
        }
    }

    #[test]
    fn go_openapi_generator_profile_emits_compat_client_surface_without_aliases() {
        if !gofmt_available() {
            eprintln!("skipping Go profile compat client test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Goals".to_string());
        }
        let files = generate_files_with_profile(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();
        for name in [
            "api_goals.go",
            "client.go",
            "configuration.go",
            "errors.go",
            "utils.go",
        ] {
            assert!(
                files.iter().any(|file| file.name == name),
                "missing {name}: {files:#?}"
            );
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test constructs a complete binary route and verifies the generated compatibility surface"
    )]
    fn go_openapi_generator_profile_emits_grouped_requests_binary_and_scoped_headers() {
        if !gofmt_available() {
            eprintln!("skipping Go profile scoped compat test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        graph.security = vec![
            SecurityScheme {
                id: "ActiveSchoolAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: "X-Plint-School-Id".to_string(),
                global: false,
            },
            SecurityScheme {
                id: "CSRFAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: "X-CSRF-Token".to_string(),
                global: false,
            },
        ];
        graph.operations[0].id = "getCourseworkSubmissionAttachment".to_string();
        graph.operations[0].handler = "getCourseworkSubmissionAttachment".to_string();
        graph.operations[0].group = Some("Coursework".to_string());
        graph.operations[0].method = "GET".to_string();
        graph.operations[0].path =
            "/coursework/assignments/{assignmentId}/submissions/{studentPersonId}/attachment"
                .to_string();
        graph.operations[0].params = vec![
            Param {
                name: "assignmentId".to_string(),
                location: "path".to_string(),
                required: true,
                schema: Type::Primitive(Prim::String),
                default: None,
                provenance: SourceSpan {
                    file: "/root/http.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            },
            Param {
                name: "studentPersonId".to_string(),
                location: "path".to_string(),
                required: true,
                schema: Type::Primitive(Prim::String),
                default: None,
                provenance: SourceSpan {
                    file: "/root/http.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            },
        ];
        graph.operations[0].request_body = None;
        graph.operations[0].responses = vec![Response {
            status: 200,
            body: None,
            body_kind: "binary".to_string(),
            content_type: Some("application/octet-stream".to_string()),
            content_types: vec!["application/octet-stream".to_string()],
        }];
        graph.operations[0].security = vec!["ActiveSchoolAuth".to_string(), "CSRFAuth".to_string()];
        graph.operations[1].security.clear();

        let files = generate_files_with_profile(
            &graph,
            "plintsdk",
            "/v1",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();

        for name in [
            "api_coursework.go",
            "client.go",
            "configuration.go",
            "errors.go",
            "utils.go",
        ] {
            assert!(
                files.iter().any(|file| file.name == name),
                "missing {name}: {files:#?}"
            );
        }

        let api = files
            .iter()
            .find(|file| file.name == "api_coursework.go")
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "func (a *CourseworkAPIService) GetCourseworkSubmissionAttachment(ctx context.Context, assignmentID string, studentPersonID string) ApiGetCourseworkSubmissionAttachmentRequest",
            "func (r ApiGetCourseworkSubmissionAttachmentRequest) Execute() ([]byte, *http.Response, error)",
            "compatApplyAPIKey(req, r.ctx, \"ActiveSchoolAuth\", \"X-Plint-School-Id\")",
            "compatApplyAPIKey(req, r.ctx, \"CSRFAuth\", \"X-CSRF-Token\")",
            "req.Header.Set(\"Accept\", \"application/octet-stream\")",
            "localVarReturnValue = localVarBody",
            "&GenericOpenAPIError{body: localVarBody, error: resp.Status}",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }
        for forbidden in [
            "req.Header.Get(\"Authorization\") != \"\"",
            "req.Header.Set(\"X-CSRF-Token\", req.Header.Get(\"Authorization\"))",
            "req.Header.Set(\"X-Plint-School-Id\", req.Header.Get(\"Authorization\"))",
        ] {
            assert!(!api.contains(forbidden), "forbidden {forbidden}:\n{api}");
        }

        let utils = files
            .iter()
            .find(|file| file.name == "utils.go")
            .unwrap()
            .contents
            .as_str();
        assert!(
            utils.contains("func parameterAddToHeaderOrQuery("),
            "{utils}"
        );

        let errors = files
            .iter()
            .find(|file| file.name == "errors.go")
            .unwrap()
            .contents
            .as_str();
        assert!(
            errors.contains("type GenericOpenAPIError struct"),
            "{errors}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_scopes_request_builder_setters_to_operation() {
        if !gofmt_available() {
            eprintln!("skipping operation-scoped compat test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        graph.operations[0].handler = "healthzGet".to_string();
        graph.operations[0].id = "healthzGet".to_string();
        graph.operations[0].group = Some("health".to_string());
        graph.operations[0].method = "GET".to_string();
        graph.operations[0].path = "/healthz".to_string();
        graph.operations[0].params.clear();
        graph.operations[0].request_body = None;

        let files = generate_files_with_profile(
            &graph,
            "plintsdk",
            "/",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.name == "api_health.go")
            .unwrap()
            .contents
            .as_str();

        assert!(api.contains("type ApiHealthzGetRequest struct"), "{api}");
        for forbidden in [
            "body any",
            "file any",
            "extraQuery map[string]any",
            "func (r ApiHealthzGetRequest) Body(",
            "func (r ApiHealthzGetRequest) Aggregation(",
            "func (r ApiHealthzGetRequest) File(",
        ] {
            assert!(!api.contains(forbidden), "forbidden {forbidden}:\n{api}");
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test constructs a representative multipart operation graph inline"
    )]
    fn go_openapi_generator_profile_uses_named_multipart_file_field() {
        if !gofmt_available() {
            eprintln!("skipping multipart compat test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        graph.operations[0].handler = "executeImportJob".to_string();
        graph.operations[0].id = "executeImportJob".to_string();
        graph.operations[0].group = Some("school".to_string());
        graph.operations[0].method = "POST".to_string();
        graph.operations[0].path = "/import-jobs/{jobId}/execute".to_string();
        graph.operations[0].params = vec![Param {
            name: "jobId".to_string(),
            location: "path".to_string(),
            required: true,
            schema: Type::Primitive(Prim::String),
            default: None,
            provenance: SourceSpan {
                file: "/root/http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        }];
        graph.operations[0].request_body = Some(SchemaRef {
            ref_id: "__synthetic.ExecuteImportJobFormRequest".to_string(),
        });
        graph.operations[0].request_body_content_type = Some("multipart/form-data".to_string());
        graph.schemas.push(Schema {
            id: "__synthetic.ExecuteImportJobFormRequest".to_string(),
            name: "ExecuteImportJobFormRequest".to_string(),
            body: Type::Object(vec![
                Field {
                    json_name: "bundle".to_string(),
                    required: true,
                    optional: false,
                    nullable: false,
                    schema: Type::Primitive(Prim::Bytes),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                },
                Field {
                    json_name: "sourceKey".to_string(),
                    required: false,
                    optional: true,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                },
                Field {
                    json_name: "sourceSystem".to_string(),
                    required: false,
                    optional: true,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                },
                Field {
                    json_name: "runAt".to_string(),
                    required: false,
                    optional: true,
                    nullable: true,
                    schema: Type::WellKnown(WellKnown::DateTime),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                },
            ]),
            enum_source_order: Vec::new(),
            provenance: SourceSpan {
                file: "/root/http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        });

        let files = generate_files_with_profile(
            &graph,
            "plintsdk",
            "/v1",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.name == "api_school.go")
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "func (r ApiExecuteImportJobRequest) Bundle(bundle any) ApiExecuteImportJobRequest",
            "func (r ApiExecuteImportJobRequest) SourceKey(sourceKey string) ApiExecuteImportJobRequest",
            "func (r ApiExecuteImportJobRequest) SourceSystem(sourceSystem string) ApiExecuteImportJobRequest",
            "func (r ApiExecuteImportJobRequest) RunAt(runAt *time.Time) ApiExecuteImportJobRequest",
            "compatMultipartFileBody(\"bundle\", r.file, r.extraQuery)",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }
        assert!(api.contains("\"time\""), "{api}");
        let utils = files
            .iter()
            .find(|file| file.name == "utils.go")
            .unwrap()
            .contents
            .as_str();
        assert!(
            utils.contains("writer.CreateFormFile(fileField, filepath.Base(reader.Name()))"),
            "{utils}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_preserves_literal_file_multipart_field_name() {
        if !gofmt_available() {
            eprintln!("skipping literal file multipart compat test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        graph.operations[0].handler = "uploadDocument".to_string();
        graph.operations[0].id = "uploadDocument".to_string();
        graph.operations[0].group = Some("documents".to_string());
        graph.operations[0].method = "POST".to_string();
        graph.operations[0].path = "/documents/{documentId}/upload".to_string();
        graph.operations[0].params = vec![Param {
            name: "documentId".to_string(),
            location: "path".to_string(),
            required: true,
            schema: Type::Primitive(Prim::String),
            default: None,
            provenance: SourceSpan {
                file: "/root/http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        }];
        graph.operations[0].request_body = Some(SchemaRef {
            ref_id: "__synthetic.UploadDocumentFormRequest".to_string(),
        });
        graph.operations[0].request_body_content_type = Some("multipart/form-data".to_string());
        graph.schemas.push(Schema {
            id: "__synthetic.UploadDocumentFormRequest".to_string(),
            name: "UploadDocumentFormRequest".to_string(),
            body: Type::Object(vec![Field {
                json_name: "file".to_string(),
                required: true,
                optional: false,
                nullable: false,
                schema: Type::Primitive(Prim::Bytes),
                description: None,
                example: None,
                meta: FieldMeta::default(),
            }]),
            enum_source_order: Vec::new(),
            provenance: SourceSpan {
                file: "/root/http.go".to_string(),
                start_line: 1,
                end_line: 1,
            },
        });

        let files = generate_files_with_profile(
            &graph,
            "plintsdk",
            "/v1",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.name == "api_documents.go")
            .unwrap()
            .contents
            .as_str();

        assert!(
            api.contains(
                "func (r ApiUploadDocumentRequest) File(file any) ApiUploadDocumentRequest"
            ),
            "{api}"
        );
        assert!(
            api.contains("compatMultipartFileBody(\"file\", r.file, r.extraQuery)"),
            "{api}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_honors_compatibility_options() {
        if !gofmt_available() {
            eprintln!("skipping Go compatibility options test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        let Type::Object(fields) = &mut graph.schemas[1].body else {
            panic!("CreateGoalInput must be an object");
        };
        fields[1].required = true;
        fields[1].optional = false;

        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.error_model = Some("CommandMessage".to_string());
        options.query_time_format = QueryTimeFormat::DateOnlyAtMidnightElseRfc3339;
        options.required_pointer_constructor_policy = RequiredPointerConstructorPolicy::ValueParam;

        let files = generate_files_with_profile_options(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();

        let errors = files
            .iter()
            .find(|file| file.name == "errors.go")
            .unwrap()
            .contents
            .as_str();
        assert!(errors.contains("var model CommandMessage"), "{errors}");
        assert!(
            errors.contains("json.Unmarshal(e.body, &model)"),
            "{errors}"
        );

        let utils = files
            .iter()
            .find(|file| file.name == "utils.go")
            .unwrap()
            .contents
            .as_str();
        assert!(
            utils.contains("parts = append(parts, compatQueryValue(v.Index(i).Interface()))"),
            "{utils}"
        );
        assert!(utils.contains("return t.Format(\"2006-01-02\")"), "{utils}");
        assert!(utils.contains("return t.Format(time.RFC3339)"), "{utils}");

        let model = files
            .iter()
            .find(|file| file.name == "model_create_goal_input.go")
            .unwrap()
            .contents
            .as_str();
        assert!(
            model.contains(
                "func NewCreateGoalInput(name string, targetDirection TargetDirection) *CreateGoalInput"
            ),
            "{model}"
        );
        assert!(
            model.contains("this.TargetDirection = &targetDirection"),
            "{model}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_emits_request_builder_aliases() {
        if !gofmt_available() {
            eprintln!("skipping request builder aliases test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        graph.operations[0].handler = "createThingPost".to_string();
        graph.operations[0].id = "createThingPost".to_string();
        graph.operations[0].group = Some("thing".to_string());
        graph.operations[1].handler = "updateThingPost".to_string();
        graph.operations[1].id = "updateThingPost".to_string();
        graph.operations[1].group = Some("thing".to_string());

        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.request_builder_aliases = GoRequestBuilderAliases::new()
            .body("ApiCreateThingPostRequest", "Thing")
            .body("ApiCreateThingPostRequest", "Input")
            .query("ApiUpdateThingPostRequest", "BranchId", "branchId");

        let files = generate_files_with_profile_options(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.name == "api_thing.go")
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "func (r ApiCreateThingPostRequest) Thing(thing any) ApiCreateThingPostRequest",
            "func (r ApiCreateThingPostRequest) Input(input any) ApiCreateThingPostRequest",
            "func (r ApiUpdateThingPostRequest) BranchId(branchId any) ApiUpdateThingPostRequest",
            "r.extraQuery[\"branchId\"] = branchId",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }
    }

    #[test]
    fn go_openapi_generator_profile_resolves_operation_selectors_for_request_builder_aliases() {
        if !gofmt_available() {
            eprintln!(
                "skipping operation selector request builder aliases test: gofmt unavailable"
            );
            return;
        }
        let mut graph = sample_graph();
        graph.operations[0].handler = "createThingPost".to_string();
        graph.operations[0].id = "createThingPost".to_string();
        graph.operations[0].group = Some("thing".to_string());
        graph.operations[1].handler = "updateThingPost".to_string();
        graph.operations[1].id = "updateThingPost".to_string();
        graph.operations[1].group = Some("thing".to_string());

        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.request_builder_aliases = GoRequestBuilderAliases::new()
            .operation("POST", "/")
            .body("Thing")
            .operation_id("updateThingPost")
            .query("BranchId", "branchId");

        let files = generate_files_with_profile_options(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.name == "api_thing.go")
            .unwrap()
            .contents
            .as_str();

        assert!(
            api.contains(
                "func (r ApiCreateThingPostRequest) Thing(thing any) ApiCreateThingPostRequest"
            ),
            "{api}"
        );
        assert!(
            api.contains("func (r ApiUpdateThingPostRequest) BranchId(branchId any) ApiUpdateThingPostRequest"),
            "{api}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_resolves_query_and_execute_selectors() {
        if !gofmt_available() {
            eprintln!("skipping query and execute selector test: gofmt unavailable");
            return;
        }
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.query_setter_argument_policy =
            GoQuerySetterArgumentPolicy::typed().any_for_query("aggregation");
        options.execute_compatibility =
            GoExecuteCompatibility::preserve_legacy().route("GET", "/list");

        let files = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.contents.contains("type ApiListGoalsRequest struct"))
            .unwrap()
            .contents
            .as_str();

        assert!(
            api.contains(
                "func (r ApiListGoalsRequest) Aggregation(aggregation any) ApiListGoalsRequest"
            ),
            "{api}"
        );
        assert!(
            api.contains("func (r ApiListGoalsRequest) Execute() (*http.Response, error)"),
            "{api}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_rejects_unknown_operation_selector() {
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.request_builder_aliases = GoRequestBuilderAliases::new()
            .operation("POST", "/missing")
            .body("Thing");

        let err = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap_err();
        assert!(err.to_string().contains("operation selector"), "{err}");
    }

    #[test]
    fn go_openapi_generator_profile_rejects_ambiguous_operation_selector() {
        let mut graph = sample_graph();
        graph.operations.push(Operation {
            id: "createGoalDuplicate".to_string(),
            method: "POST".to_string(),
            path: "/".to_string(),
            handler: "createGoalDuplicate".to_string(),
            group: None,
            middleware: Vec::new(),
            params: Vec::new(),
            request_body: None,
            request_body_required: true,
            request_body_content_type: None,
            responses: Vec::new(),
            security: Vec::new(),
            security_overrides_global: false,
            provenance: SourceSpan {
                file: "/root/http.go".to_string(),
                start_line: 99,
                end_line: 99,
            },
        });

        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.request_builder_aliases = GoRequestBuilderAliases::new()
            .operation("POST", "/")
            .body("Thing");

        let err = generate_files_with_profile_options(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap_err();
        assert!(err.to_string().contains("matched 2 operations"), "{err}");
    }

    #[test]
    fn go_openapi_generator_profile_body_alias_wins_over_generated_body_field_setter() {
        if !gofmt_available() {
            eprintln!("skipping body alias precedence test: gofmt unavailable");
            return;
        }
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.request_builder_aliases =
            GoRequestBuilderAliases::new().body("ApiCreateGoalRequest", "Name");

        let files = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.contents.contains("type ApiCreateGoalRequest struct"))
            .unwrap()
            .contents
            .as_str();

        assert!(
            api.contains("func (r ApiCreateGoalRequest) Name(name any) ApiCreateGoalRequest"),
            "{api}"
        );
        assert!(api.contains("r.body = name"), "{api}");
        assert!(!api.contains("r.extraQuery[\"name\"] = name"), "{api}");
        assert!(
            !api.contains("r.body = compatSetBodyField(r.body, \"name\", name)"),
            "{api}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_body_field_setters_update_json_body_not_query() {
        if !gofmt_available() {
            eprintln!("skipping body field setter test: gofmt unavailable");
            return;
        }
        let files = generate_files_with_profile(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.contents.contains("type ApiCreateGoalRequest struct"))
            .unwrap()
            .contents
            .as_str();

        assert!(
            api.contains("func (r ApiCreateGoalRequest) Name(name any) ApiCreateGoalRequest"),
            "{api}"
        );
        assert!(
            api.contains("r.body = compatSetBodyField(r.body, \"name\", name)"),
            "{api}"
        );
        assert!(!api.contains("r.extraQuery[\"name\"] = name"), "{api}");
    }

    #[test]
    fn go_openapi_generator_profile_encodes_form_urlencoded_body_fields() {
        if !gofmt_available() {
            eprintln!("skipping form-url-encoded compat test: gofmt unavailable");
            return;
        }
        let mut graph = sample_graph();
        graph.operations[0].request_body_content_type =
            Some("application/x-www-form-urlencoded".to_string());

        let files = generate_files_with_profile(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &SdkProfile::go_openapi_generator_compat(),
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.contents.contains("type ApiCreateGoalRequest struct"))
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "func (r ApiCreateGoalRequest) Name(name any) ApiCreateGoalRequest",
            "r.body = compatSetBodyField(r.body, \"name\", name)",
            "encodedBody, err := compatEncodeFormBody(bodyValue)",
            "reqContentType = \"application/x-www-form-urlencoded\"",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }

        let utils = files
            .iter()
            .find(|file| file.name == "utils.go")
            .unwrap()
            .contents
            .as_str();
        assert!(utils.contains("func compatEncodeFormBody("), "{utils}");
        assert!(utils.contains("func compatSetBodyField("), "{utils}");
    }

    #[test]
    fn go_openapi_generator_profile_can_emit_any_query_setters() {
        if !gofmt_available() {
            eprintln!("skipping any query setters test: gofmt unavailable");
            return;
        }
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.query_setter_argument_policy = GoQuerySetterArgumentPolicy::Any;

        let files = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.contents.contains("type ApiListGoalsRequest struct"))
            .unwrap()
            .contents
            .as_str();
        assert!(
            api.contains(
                "func (r ApiListGoalsRequest) Aggregation(aggregation any) ApiListGoalsRequest"
            ),
            "{api}"
        );
        assert!(
            api.contains("r.extraQuery[\"aggregation\"] = aggregation"),
            "{api}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_can_widen_selected_query_setters_to_any() {
        if !gofmt_available() {
            eprintln!("skipping selected any query setters test: gofmt unavailable");
            return;
        }
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.query_setter_argument_policy =
            GoQuerySetterArgumentPolicy::typed().any_for("ApiListGoalsRequest", "Aggregation");
        let mut graph = sample_graph();
        graph
            .operations
            .iter_mut()
            .find(|op| op.id == "listGoals")
            .unwrap()
            .params
            .push(Param {
                name: "cursor".to_string(),
                location: "query".to_string(),
                required: false,
                schema: Type::Primitive(Prim::String),
                default: None,
                provenance: SourceSpan {
                    file: "/root/h.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            });

        let files = generate_files_with_profile_options(
            &graph,
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.contents.contains("type ApiListGoalsRequest struct"))
            .unwrap()
            .contents
            .as_str();

        assert!(
            api.contains(
                "func (r ApiListGoalsRequest) Aggregation(aggregation any) ApiListGoalsRequest"
            ),
            "{api}"
        );
        assert!(
            api.contains("r.extraQuery[\"aggregation\"] = aggregation"),
            "{api}"
        );
        assert!(
            api.contains("func (r ApiListGoalsRequest) Cursor(cursor string) ApiListGoalsRequest"),
            "{api}"
        );
        assert!(!api.contains("r.extraQuery[\"cursor\"] = cursor"), "{api}");
    }

    #[test]
    fn go_openapi_generator_profile_rejects_unknown_selected_any_query_setter() {
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.query_setter_argument_policy =
            GoQuerySetterArgumentPolicy::typed().any_for("ApiListGoalsRequest", "Missing");

        let err = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("unknown query setter 'Missing'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_preserves_selected_legacy_execute() {
        if !gofmt_available() {
            eprintln!("skipping execute compatibility test: gofmt unavailable");
            return;
        }
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.execute_compatibility =
            GoExecuteCompatibility::preserve_legacy().request("ApiListGoalsRequest");

        let files = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap();
        let list_api = files
            .iter()
            .find(|file| file.contents.contains("type ApiListGoalsRequest struct"))
            .unwrap()
            .contents
            .as_str();
        assert!(
            list_api.contains("func (r ApiListGoalsRequest) Execute() (*http.Response, error)"),
            "{list_api}"
        );
        assert!(
            list_api.contains("_, resp, err := r.ExecuteTyped()"),
            "{list_api}"
        );
        assert!(
            list_api.contains("func (r ApiListGoalsRequest) ExecuteTyped() (*ListGoalsOutput, *http.Response, error)"),
            "{list_api}"
        );

        let create_api = files
            .iter()
            .find(|file| file.contents.contains("type ApiCreateGoalRequest struct"))
            .unwrap()
            .contents
            .as_str();
        assert!(
            create_api.contains(
                "func (r ApiCreateGoalRequest) Execute() (*CommandMessage, *http.Response, error)"
            ),
            "{create_api}"
        );
        assert!(!create_api.contains("ExecuteTyped()"), "{create_api}");
    }

    #[test]
    fn go_openapi_generator_profile_rejects_unknown_execute_selector() {
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.execute_compatibility =
            GoExecuteCompatibility::preserve_legacy().request("ApiMissingRequest");

        let err = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown request builder"), "{err}");
    }

    #[test]
    fn go_openapi_generator_profile_rejects_conflicting_request_builder_aliases() {
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.request_builder_aliases = GoRequestBuilderAliases::new()
            .body("ApiCreateGoalRequest", "ThingID")
            .query("ApiCreateGoalRequest", "ThingId", "thingId");

        let err = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("conflicting method"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn go_openapi_generator_profile_rejects_invalid_error_model_name() {
        let profile = SdkProfile::go_openapi_generator_compat();
        let mut options = GoSdkOptions::for_profile(&profile);
        options.error_model = Some("Error Response".to_string());

        let err = generate_files_with_profile_options(
            &sample_graph(),
            "goalservice",
            "/goal",
            &SdkFileLayout::split(),
            &SdkTypeAliases::default(),
            &profile,
            options,
        )
        .unwrap_err();

        assert!(err.to_string().contains("error_model"), "{err}");
    }
}
