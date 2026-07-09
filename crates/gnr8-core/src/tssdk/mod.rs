//! TypeScript SDK generation seam (Phase 5): generates a dependency-free TypeScript SDK from the API
//! graph.
//!
//! [`generate`] turns the Phase-2 [`crate::graph::ApiGraph`] into a single deterministic,
//! dependency-free TypeScript SDK bundle String (D-06): an `index.ts` re-export surface, a `client.ts`
//! (an injectable platform-`fetch`-backed `Client` plus one method per operation), a typed `errors.ts`
//! (`ApiError extends Error`), and a `models.ts` (`export interface` request/response models +
//! string-literal-union named enums + `export type` aliases).
//!
//! This is the structural twin of [`crate::pysdk`], MINUS the Python-only workarounds (required-first
//! field ordering, the `from __future__` header, PEP-484 forward-ref aliases, the f-string `safe=''`
//! trick): TypeScript `?:` is order-free, `type` aliases are order-independent, and template literals
//! impose no backslash restriction. Each file is framed into a [`bundle::SdkBundle`] with stable file
//! markers; the pipeline is byte-identical across runs and never panics (RUST-04). [`write_to_dir`]
//! materializes the same framing.

mod emit;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::graph::{ApiGraph, Field, Operation, Prim, Type};
use crate::sdk::bundle::{check_unique_file_names, SdkBundle, SdkFile};
use crate::sdk::emit_common::{
    api_key_credential_names, check_unique_schema_names, file_in_dir, file_stem,
    http_auth_features, join_path, model_file_name, operation_api_key_schemes, operation_file_name,
    operation_group_file_name, operation_group_name, path_tokens, path_tokens_match,
    quoted_string_literal, request_body_model_of, split_words, success_responses_of,
    validate_sdk_base_path, ApiKeyLocation,
};
use crate::sdk::layout::{OperationFileSplit, SdkFileLayout};
use crate::sdk::profile::SdkProfile;
use crate::sdk::surface::SdkTypeAliases;
use crate::sdk::typescript::{TsBarrelExports, TsResponsePolicy, TsSdkOptions};

/// Generate the TypeScript SDK as a deterministic, dependency-free multi-file bundle String (D-06,
/// TSSDK-01).
///
/// Emits `client.ts` (the `fetch`-backed `Client` + one method per operation), `errors.ts` (typed
/// `ApiError`), `index.ts` (re-exports), and `models.ts` (`interface` models + literal-union enums +
/// `type` aliases) in a FIXED alpha push order, then frames them into a single [`bundle::SdkBundle`]
/// String. Generating twice over the same graph is byte-identical (TSSDK-03). There is NO `gofmt`-style
/// normalization step (the generated TypeScript is already correct) and NO computed import header.
///
/// `package` is the SDK's package name (derived from the `TsSdk` target's module path, the single source
/// of truth — wired in plan 02). `base_path` is the API base/mount path joined to each operation's
/// group-relative path in the emitted request URLs — the SAME single source of truth (the graph's
/// `base_path`) the `OpenAPI` lowering and the Go/Python SDKs take it from (CLAUDE.md rules 3 & 4).
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] for an un-representable graph fact (a dangling `$ref`, an inline
/// object, a path whose templated tokens do not match its declared path params, a duplicate schema name,
/// or a `fmt` write error folded by the emitters' `sink`).
pub fn generate(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
) -> Result<String, crate::CoreError> {
    generate_with_layout(graph, package, base_path, &SdkFileLayout::compact())
}

/// Generate the TypeScript SDK with a configurable file layout.
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
    let options = TsSdkOptions::strict();
    generate_files_with_layout_options(graph, package, base_path, layout, aliases, &options)
}

#[expect(
    clippy::too_many_lines,
    reason = "SDK generation orchestration keeps file ordering, split layout, and profile options in one deterministic pass"
)]
fn generate_files_with_layout_options(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    options: &TsSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    validate_sdk_base_path(base_path)?;
    check_unique_schema_names(graph, "TypeScript SDK")?;

    let mut files: Vec<SdkFile> = Vec::new();
    let auth_credentials = api_key_credential_names(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;

    // Fixed alpha push order: client.ts, errors.ts, index.ts, models.ts — the D-06 frame order the
    // bundle locks. client.ts is the client skeleton followed by the operation methods.
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let model_dir = layout.model_dir_ref().unwrap_or("models");
    let split_operations =
        layout.is_split() && !matches!(layout.operation_split(), OperationFileSplit::Compact);
    let http_auth = http_auth_features(graph)?;
    let mut client = emit::emit_client_with_models(
        package,
        model_dir.trim_matches('/'),
        !auth_credentials.is_empty(),
        http_auth.bearer,
        http_auth.basic,
    );
    if split_operations {
        client.push_str(&emit::emit_split_operation_surface(&ops)?);
        client.push_str(&emit_operation_module_imports(layout, graph)?);
    } else {
        client.push_str(&emit::emit_operations(graph, package, base_path, &ops)?);
    }
    files.push(SdkFile {
        name: "client.ts".to_string(),
        contents: client,
    });

    files.push(SdkFile {
        name: "errors.ts".to_string(),
        contents: emit::emit_errors(package),
    });

    files.push(SdkFile {
        name: "index.ts".to_string(),
        contents: emit::emit_index_with_models(
            graph,
            package,
            model_dir.trim_matches('/'),
            &resolved_aliases,
        )?,
    });

    if split_operations {
        files.extend(generate_operation_files(
            graph,
            base_path,
            layout,
            model_dir.trim_matches('/'),
        )?);
    }

    if layout.is_split() {
        let model_index_name = file_in_dir(Some(model_dir), "index.ts");
        let mut model_exports = Vec::new();
        let mut schema_file_names = BTreeMap::new();
        for schema in &graph.schemas {
            let default_name =
                file_in_dir(Some(model_dir), &format!("{}.ts", file_stem(&schema.name)));
            let name = if layout.model_file_template_ref().is_some() {
                ts_model_file_name(layout, schema, &format!("{}.ts", file_stem(&schema.name)))?
            } else {
                default_name
            };
            model_exports.push(ts_relative_module(&model_index_name, &name));
            schema_file_names.insert(schema.name.clone(), name);
        }
        for alias in &resolved_aliases {
            let name = file_in_dir(Some(model_dir), &format!("{}.ts", file_stem(&alias.alias)));
            validate_ts_file_name(&name)?;
            model_exports.push(ts_relative_module(&model_index_name, &name));
        }
        files.push(SdkFile {
            name: model_index_name.clone(),
            contents: emit_ts_models_index(&model_exports),
        });
        for schema in &graph.schemas {
            let name = schema_file_names
                .get(&schema.name)
                .ok_or_else(|| crate::CoreError::SdkGen {
                    message: format!(
                        "schema {} did not have a precomputed TypeScript file",
                        schema.name
                    ),
                })?
                .clone();
            let models_module = ts_relative_module(&name, &model_index_name);
            files.push(SdkFile {
                name,
                contents: emit::emit_model_schema_with_policies(
                    graph,
                    schema,
                    &models_module,
                    options.model_properties,
                    options.nullable,
                )?,
            });
        }
        for alias in &resolved_aliases {
            let name = file_in_dir(Some(model_dir), &format!("{}.ts", file_stem(&alias.alias)));
            validate_ts_file_name(&name)?;
            let canonical = schema_file_names.get(&alias.canonical).ok_or_else(|| {
                crate::CoreError::SdkGen {
                    message: format!(
                        "type alias {} references unknown canonical model {}",
                        alias.alias, alias.canonical
                    ),
                }
            })?;
            let canonical_module = ts_relative_module(&name, canonical);
            files.push(SdkFile {
                name,
                contents: emit::emit_model_alias(alias, &canonical_module),
            });
        }
    } else {
        files.push(SdkFile {
            name: "models.ts".to_string(),
            contents: emit::emit_models_with_aliases_and_policies(
                graph,
                &resolved_aliases,
                options.model_properties,
                options.nullable,
            )?,
        });
    }

    check_unique_file_names(&files, "TypeScript SDK")?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn emit_ts_models_index(exports: &[String]) -> String {
    let mut out = String::new();
    for module in exports {
        let module = quoted_string_literal(module);
        out.push_str("export * from ");
        out.push_str(&module);
        out.push_str(";\n");
    }
    out
}

fn generate_operation_files(
    graph: &ApiGraph,
    base_path: &str,
    layout: &SdkFileLayout,
    model_module: &str,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let mut files = Vec::new();
    match layout.operation_split() {
        OperationFileSplit::Compact => {}
        OperationFileSplit::PerEndpoint => {
            for op in ops {
                let name =
                    ts_operation_file_name(layout, op, &format!("api_{}.ts", file_stem(&op.id)))?;
                files.push(SdkFile {
                    contents: emit_operation_file(graph, base_path, &[op], model_module, &name)?,
                    name,
                });
            }
        }
        OperationFileSplit::PerTag => {
            for (group, group_ops) in operation_groups(&ops) {
                let name = ts_operation_group_file_name(
                    layout,
                    &group,
                    &format!("api_{}.ts", file_stem(&group)),
                )?;
                files.push(SdkFile {
                    contents: emit_operation_file(
                        graph,
                        base_path,
                        &group_ops,
                        model_module,
                        &name,
                    )?,
                    name,
                });
            }
        }
    }
    for index in ts_barrel_files(files.iter().map(|file| file.name.as_str())) {
        files.push(SdkFile {
            name: index,
            contents: String::new(),
        });
    }
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

fn emit_operation_file(
    graph: &ApiGraph,
    base_path: &str,
    ops: &[&Operation],
    model_module: &str,
    file_name: &str,
) -> Result<String, crate::CoreError> {
    let root = ts_relative_root(file_name);
    emit::emit_operation_module(
        graph,
        base_path,
        ops,
        &format!("{root}client"),
        &format!("{root}errors"),
        &format!("{root}{model_module}"),
    )
}

fn ts_relative_root(file_name: &str) -> String {
    let depth = file_name.matches('/').count();
    if depth == 0 {
        "./".to_string()
    } else {
        "../".repeat(depth)
    }
}

fn ts_relative_module(from_file: &str, to_file: &str) -> String {
    let from_dir: Vec<&str> = from_file.rsplit_once('/').map_or(Vec::new(), |(dir, _)| {
        dir.split('/').filter(|part| !part.is_empty()).collect()
    });
    let to_without_ext = to_file.strip_suffix(".ts").unwrap_or(to_file);
    let to_parts: Vec<&str> = to_without_ext
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let common = from_dir
        .iter()
        .zip(to_parts.iter())
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts: Vec<&str> = Vec::new();
    parts.extend(std::iter::repeat_n(
        "..",
        from_dir.len().saturating_sub(common),
    ));
    parts.extend(to_parts.iter().skip(common).copied());
    if parts.first().is_some_and(|part| *part == "..") {
        parts.join("/")
    } else {
        format!("./{}", parts.join("/"))
    }
}

fn ts_operation_file_name(
    layout: &SdkFileLayout,
    op: &Operation,
    default_file_name: &str,
) -> Result<String, crate::CoreError> {
    let name = operation_file_name(layout, op, default_file_name)?;
    validate_ts_file_name(&name)?;
    Ok(name)
}

fn ts_operation_group_file_name(
    layout: &SdkFileLayout,
    group: &str,
    default_file_name: &str,
) -> Result<String, crate::CoreError> {
    let name = operation_group_file_name(layout, group, default_file_name)?;
    validate_ts_file_name(&name)?;
    Ok(name)
}

fn ts_model_file_name(
    layout: &SdkFileLayout,
    schema: &crate::graph::Schema,
    default_file_name: &str,
) -> Result<String, crate::CoreError> {
    let name = model_file_name(layout, schema, default_file_name)?;
    validate_ts_file_name(&name)?;
    Ok(name)
}

fn validate_ts_file_name(name: &str) -> Result<(), crate::CoreError> {
    if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
    {
        return Ok(());
    }
    Err(crate::CoreError::SdkGen {
        message: format!("TypeScript SDK split file {name:?} must end with .ts"),
    })
}

fn emit_operation_module_imports(
    layout: &SdkFileLayout,
    graph: &ApiGraph,
) -> Result<String, crate::CoreError> {
    let mut out = String::new();
    let mut client_interface = String::new();
    let mut assignments = String::new();
    for (index, (file, methods)) in operation_file_methods(layout, graph)?
        .into_iter()
        .enumerate()
    {
        let module = file.trim_end_matches(".ts");
        for (method_index, method) in methods.into_iter().enumerate() {
            let binding = format!("operation{index}_{method_index}");
            let specifier = quoted_string_literal(&format!("./{module}"));
            out.push_str("import { ");
            out.push_str(&method);
            out.push_str(" as ");
            out.push_str(&binding);
            out.push_str(" } from ");
            out.push_str(&specifier);
            out.push_str(";\n");
            client_interface.push_str("  ");
            client_interface.push_str(&method);
            client_interface.push_str(": typeof ");
            client_interface.push_str(&binding);
            client_interface.push_str(";\n");
            assignments.push_str("Client.prototype.");
            assignments.push_str(&method);
            assignments.push_str(" = ");
            assignments.push_str(&binding);
            assignments.push_str(";\n");
        }
    }
    if !client_interface.is_empty() {
        out.push_str("\nexport interface Client {\n");
        out.push_str(&client_interface);
        out.push_str("}\n");
    }
    if !assignments.is_empty() {
        out.push('\n');
        out.push_str(&assignments);
    }
    Ok(out)
}

fn operation_file_methods(
    layout: &SdkFileLayout,
    graph: &ApiGraph,
) -> Result<Vec<(String, Vec<String>)>, crate::CoreError> {
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let mut files = Vec::new();
    match layout.operation_split() {
        OperationFileSplit::Compact => {}
        OperationFileSplit::PerEndpoint => {
            for op in ops {
                files.push((
                    ts_operation_file_name(layout, op, &format!("api_{}.ts", file_stem(&op.id)))?,
                    vec![emit::operation_method_name(op)],
                ));
            }
        }
        OperationFileSplit::PerTag => {
            for (group, ops) in operation_groups(&ops) {
                files.push((
                    ts_operation_group_file_name(
                        layout,
                        &group,
                        &format!("api_{}.ts", file_stem(&group)),
                    )?,
                    ops.into_iter().map(emit::operation_method_name).collect(),
                ));
            }
        }
    }
    Ok(files)
}

fn ts_barrel_files<'a>(file_names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut indexes = Vec::new();
    for name in file_names {
        let Some((dir, _)) = name.rsplit_once('/') else {
            continue;
        };
        let index = format!("{dir}/index.ts");
        if !indexes.contains(&index) {
            indexes.push(index);
        }
    }
    indexes
}

#[cfg(test)]
pub(crate) fn generate_files_with_profile(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    profile: &SdkProfile,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    let options = TsSdkOptions::for_profile(profile);
    generate_files_with_profile_options(
        graph, package, base_path, layout, aliases, profile, &options,
    )
}

pub(crate) fn generate_files_with_profile_options(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    profile: &SdkProfile,
    options: &TsSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    if !profile.is_typescript_axios_compat() && options.response.is_axios_response_wrapper() {
        return Err(crate::CoreError::Config {
            message: "TsResponsePolicy::AxiosResponseWrapper requires SdkProfile::typescript_axios_compat()"
                .to_string(),
        });
    }
    if profile.is_minimal() {
        return generate_files_with_layout_options(
            graph, package, base_path, layout, aliases, options,
        );
    }
    if profile.is_typescript_fetch_compat() {
        return generate_typescript_fetch_compat_files(graph, package, base_path, aliases, options);
    }
    if profile.is_typescript_axios_compat() {
        return generate_openapi_generator_compat_files(
            graph, package, base_path, aliases, options,
        );
    }
    generate_files_with_layout_options(graph, package, base_path, layout, aliases, options)
}

fn generate_openapi_generator_compat_files(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    aliases: &SdkTypeAliases,
    options: &TsSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    validate_sdk_base_path(base_path)?;
    check_unique_schema_names(graph, "TypeScript SDK")?;

    let resolved_aliases = aliases.resolve(graph)?;
    let mut files = vec![
        SdkFile {
            name: "api.ts".to_string(),
            contents: emit_axios_api(graph, base_path, options.response)?,
        },
        SdkFile {
            name: "base.ts".to_string(),
            contents: emit_axios_base(),
        },
        SdkFile {
            name: "configuration.ts".to_string(),
            contents: emit_axios_configuration(),
        },
        SdkFile {
            name: "errors.ts".to_string(),
            contents: emit::emit_errors(package),
        },
        SdkFile {
            name: "index.ts".to_string(),
            contents: emit_axios_index(),
        },
        SdkFile {
            name: "models.ts".to_string(),
            contents: emit::emit_models_openapi_generator_compat(
                graph,
                package,
                &resolved_aliases,
                options.model_properties,
                options.nullable,
            )?,
        },
        SdkFile {
            name: "package.json".to_string(),
            contents: emit_axios_package_json(package),
        },
    ];
    check_unique_file_names(&files, "TypeScript SDK")?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn generate_typescript_fetch_compat_files(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    aliases: &SdkTypeAliases,
    options: &TsSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    validate_sdk_base_path(base_path)?;
    check_unique_schema_names(graph, "TypeScript SDK")?;

    let resolved_aliases = aliases.resolve(graph)?;
    let mut files = vec![
        SdkFile {
            name: "apis/index.ts".to_string(),
            contents: emit_fetch_api(graph, base_path, options)?,
        },
        SdkFile {
            name: "index.ts".to_string(),
            contents: emit_fetch_index(graph, options.barrel_exports),
        },
        SdkFile {
            name: "models/index.ts".to_string(),
            contents: emit::emit_models_openapi_generator_compat(
                graph,
                package,
                &resolved_aliases,
                options.model_properties,
                options.nullable,
            )?,
        },
        SdkFile {
            name: "runtime.ts".to_string(),
            contents: emit_fetch_runtime(options.init_override_function),
        },
        SdkFile {
            name: "package.json".to_string(),
            contents: emit_fetch_package_json(package),
        },
    ];
    check_unique_file_names(&files, "TypeScript SDK")?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn emit_fetch_index(graph: &ApiGraph, exports: TsBarrelExports) -> String {
    match exports {
        TsBarrelExports::Star => "\
export * from \"./runtime\";
export * from \"./apis\";
export * from \"./models\";
"
        .to_string(),
        TsBarrelExports::OpenApiGeneratorCompat => {
            let mut out = String::from("export * from \"./runtime\";\n");
            let classes: Vec<String> = grouped_operations(graph)
                .into_keys()
                .map(|service| api_class_name(&service))
                .collect();
            if !classes.is_empty() {
                out.push_str("export {\n");
                for class in classes {
                    out.push_str("  ");
                    out.push_str(&class);
                    out.push_str(",\n");
                }
                out.push_str("} from \"./apis\";\n");
            }
            out.push_str("export * from \"./models\";\n");
            out
        }
    }
}

fn emit_fetch_package_json(package: &str) -> String {
    format!(
        "{{
  \"name\": {},
  \"version\": \"0.1.0\",
  \"type\": \"module\",
  \"main\": \"./index.js\",
  \"types\": \"./index.d.ts\"
}}
",
        quoted_string_literal(package)
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "the typescript-fetch compatibility runtime is emitted as one generated runtime.ts file"
)]
fn emit_fetch_runtime(init_override_function: bool) -> String {
    let mut runtime = "\
export type HTTPHeaders = Record<string, string>;
export type HTTPQuery = Record<string, unknown>;
export interface HTTPRequestInit extends RequestInit {
  headers?: HTTPHeaders;
}
export type FetchAPI = typeof fetch;
export type HTTPMethod = \"GET\" | \"POST\" | \"PUT\" | \"PATCH\" | \"DELETE\" | \"HEAD\" | \"OPTIONS\";
export type ApiKey = string | ((name: string) => string | Promise<string>);

export interface ConfigurationParameters {
  basePath?: string;
  fetchApi?: typeof fetch;
  middleware?: Middleware[];
  headers?: HTTPHeaders;
  credentials?: RequestCredentials;
  apiKey?: ApiKey;
  apiKeys?: Record<string, ApiKey>;
}

export class Configuration {
  public readonly basePath: string;
  public readonly fetchApi: typeof fetch;
  public readonly middleware: Middleware[];
  public readonly headers: HTTPHeaders;
  public readonly credentials?: RequestCredentials;
  private readonly apiKey?: ApiKey;
  private readonly apiKeys: Record<string, ApiKey>;

  constructor(parameters: ConfigurationParameters = {}) {
    this.basePath = (parameters.basePath ?? \"\").replace(/\\/+$/, \"\");
    this.fetchApi = parameters.fetchApi ?? fetch;
    this.middleware = parameters.middleware ?? [];
    this.headers = parameters.headers ?? {};
    this.credentials = parameters.credentials;
    this.apiKey = parameters.apiKey;
    this.apiKeys = parameters.apiKeys ?? {};
  }

  async getApiKey(...names: string[]): Promise<string | undefined> {
    for (const name of names) {
      const value = this.apiKeys[name];
      if (value !== undefined) {
        return resolveApiKey(value, name);
      }
    }
    if (this.apiKey !== undefined) {
      return resolveApiKey(this.apiKey, names[0] ?? \"apiKey\");
    }
    return undefined;
  }

  withMiddleware(...middleware: Middleware[]): Configuration {
    return new Configuration({
      basePath: this.basePath,
      fetchApi: this.fetchApi,
      middleware: this.middleware.concat(middleware),
      headers: this.headers,
      credentials: this.credentials,
      apiKey: this.apiKey,
      apiKeys: { ...this.apiKeys },
    });
  }
}

async function resolveApiKey(apiKey: ApiKey, name: string): Promise<string> {
  if (typeof apiKey === \"function\") {
    return await apiKey(name);
  }
  return apiKey;
}

export interface RequestOpts {
  path: string;
  method: HTTPMethod;
  headers?: HTTPHeaders;
  query?: HTTPQuery;
  body?: unknown;
}

export interface RequestContext {
  url: string;
  init: RequestInit;
}

export interface ResponseContext {
  response: Response;
}

export interface ErrorContext {
  error: unknown;
  url: string;
  init: RequestInit;
}

export interface Middleware {
  pre?(context: RequestContext): Promise<RequestContext | void> | RequestContext | void;
  post?(context: ResponseContext): Promise<Response | void> | Response | void;
  onError?(context: ErrorContext): Promise<Response | void> | Response | void;
}

export interface ApiResponse<T> {
  raw: Response;
  value(): Promise<T>;
}

export class JSONApiResponse<T> implements ApiResponse<T> {
  constructor(
    public readonly raw: Response,
    private readonly transformer: (json: unknown) => T | Promise<T> = (json) => json as T,
  ) {}

  async value(): Promise<T> {
    return await this.transformer(await this.raw.json());
  }
}

export class VoidApiResponse implements ApiResponse<void> {
  constructor(public readonly raw: Response) {}

  async value(): Promise<void> {
    return undefined;
  }
}

export class BlobApiResponse implements ApiResponse<Blob> {
  constructor(public readonly raw: Response) {}

  async value(): Promise<Blob> {
    return await this.raw.blob();
  }
}

export class ResponseError extends Error {
  constructor(public readonly response: Response, message?: string) {
    super(message ?? `Response returned status code ${response.status}`);
    this.name = \"ResponseError\";
  }
}

export class FetchError extends Error {
  constructor(public readonly cause: unknown, message?: string) {
    super(message ?? \"The request failed and no response was returned\");
    this.name = \"FetchError\";
  }
}

export class BaseAPI {
  protected readonly configuration: Configuration;

  constructor(configuration: Configuration = new Configuration()) {
    this.configuration = configuration;
  }

  withMiddleware(...middleware: Middleware[]): this {
    const next = Object.create(this);
    next.configuration = this.configuration.withMiddleware(...middleware);
    return next;
  }

  protected async request(context: RequestOpts, initOverrides: RequestInit = {}): Promise<Response> {
    const query = querystring(context.query ?? {});
    const url = `${this.configuration.basePath}${context.path}${query ? `?${query}` : \"\"}`;
    const headers: HTTPHeaders = {
      ...this.configuration.headers,
      ...(context.headers ?? {}),
      ...((initOverrides.headers as HTTPHeaders | undefined) ?? {}),
    };
    const init: RequestInit = {
      ...initOverrides,
      method: context.method,
      headers,
      credentials: initOverrides.credentials ?? this.configuration.credentials,
    };
    if (context.body !== undefined) {
      if (isBodyInit(context.body)) {
        init.body = context.body;
      } else {
        if (headers[\"Content-Type\"] === undefined) {
          headers[\"Content-Type\"] = \"application/json\";
        }
        init.body = JSON.stringify(context.body);
      }
    }

    let requestContext: RequestContext = { url, init };
    for (const middleware of this.configuration.middleware) {
      if (middleware.pre) {
        requestContext = (await middleware.pre(requestContext)) ?? requestContext;
      }
    }

    let response: Response | undefined;
    try {
      response = await this.configuration.fetchApi(requestContext.url, requestContext.init);
    } catch (error) {
      for (const middleware of this.configuration.middleware) {
        if (middleware.onError) {
          response = (await middleware.onError({ error, url: requestContext.url, init: requestContext.init })) ?? response;
        }
      }
      if (response === undefined) {
        throw new FetchError(error);
      }
    }

    for (const middleware of this.configuration.middleware) {
      if (middleware.post) {
        response = (await middleware.post({ response })) ?? response;
      }
    }
    return response;
  }
}

function querystring(params: HTTPQuery): string {
  const searchParams = new URLSearchParams();
  for (const key of Object.keys(params)) {
    const value = params[key];
    if (value === undefined || value === null) {
      continue;
    }
    if (Array.isArray(value)) {
      for (const item of value) {
        searchParams.append(key, String(item));
      }
    } else {
      searchParams.set(key, String(value));
    }
  }
  return searchParams.toString();
}

function isBodyInit(value: unknown): value is BodyInit {
  return typeof value === \"string\"
    || value instanceof Blob
    || value instanceof FormData
    || value instanceof URLSearchParams
    || value instanceof ArrayBuffer;
}
"
    .to_string();
    if init_override_function {
        runtime = runtime
            .replace(
                "export interface Middleware {\n",
                "export interface InitOverrideFunction {\n  (requestContext: { init: RequestInit; context: RequestOpts }): Promise<RequestInit> | RequestInit;\n}\n\nexport interface Middleware {\n",
            )
            .replace(
                "  protected async request(context: RequestOpts, initOverrides: RequestInit = {}): Promise<Response> {",
                "  protected async request(context: RequestOpts, initOverrides: RequestInit | InitOverrideFunction = {}): Promise<Response> {",
            )
            .replace(
                "    const headers: HTTPHeaders = {\n      ...this.configuration.headers,\n      ...(context.headers ?? {}),\n      ...((initOverrides.headers as HTTPHeaders | undefined) ?? {}),\n    };\n    const init: RequestInit = {\n      ...initOverrides,\n      method: context.method,\n      headers,\n      credentials: initOverrides.credentials ?? this.configuration.credentials,\n    };",
                "    const initOverrideObject = typeof initOverrides === \"function\" ? {} : initOverrides;\n    const headers: HTTPHeaders = {\n      ...this.configuration.headers,\n      ...(context.headers ?? {}),\n      ...((initOverrideObject.headers as HTTPHeaders | undefined) ?? {}),\n    };\n    const init: RequestInit = {\n      ...initOverrideObject,\n      method: context.method,\n      headers,\n      credentials: initOverrideObject.credentials ?? this.configuration.credentials,\n    };",
            )
            .replace(
                "    let requestContext: RequestContext = { url, init };",
                "    const initOverrideFn = typeof initOverrides === \"function\" ? initOverrides : undefined;\n    const finalInit = initOverrideFn ? await initOverrideFn({ init, context }) : init;\n    let requestContext: RequestContext = { url, init: finalInit };",
            );
    }
    runtime
}

fn emit_fetch_api(
    graph: &ApiGraph,
    base_path: &str,
    options: &TsSdkOptions,
) -> Result<String, crate::CoreError> {
    let mut out = String::new();
    out.push_str(
        "\
import * as runtime from \"../runtime\";
import * as models from \"../models\";

",
    );
    if graph.operations.iter().any(is_multipart_operation) {
        out.push_str(
            "\
function multipartBody(fields: Record<string, Blob | string | number | boolean | null | undefined>): FormData {
  const formData = new FormData();
  for (const name in fields) {
    const value = fields[name];
    if (value !== undefined && value !== null) {
      formData.append(name, value instanceof Blob ? value : String(value));
    }
  }
  return formData;
}

",
        );
    }
    for (service, ops) in grouped_operations(graph) {
        let class_name = api_class_name(&service);
        for op in &ops {
            emit_fetch_request_alias(&mut out, graph, op, options)?;
        }
        writeln!(out, "export class {class_name} extends runtime.BaseAPI {{")
            .map_err(ts_mod_sink)?;
        for op in &ops {
            emit_fetch_operation_methods(&mut out, graph, base_path, op, options)?;
        }
        writeln!(out, "}}\n").map_err(ts_mod_sink)?;
    }
    Ok(out)
}

fn emit_fetch_request_alias(
    out: &mut String,
    graph: &ApiGraph,
    op: &Operation,
    options: &TsSdkOptions,
) -> Result<(), crate::CoreError> {
    emit_request_alias(out, graph, op, &options.request_body_param_name)
}

#[allow(clippy::too_many_lines)]
fn emit_fetch_operation_methods(
    out: &mut String,
    graph: &ApiGraph,
    base_path: &str,
    op: &Operation,
    options: &TsSdkOptions,
) -> Result<(), crate::CoreError> {
    let method_name = emit::camel(&op.handler);
    let raw_method_name = format!("{method_name}Raw");
    let request_name = request_alias_name(op);
    let request_fields = request_fields(graph, op, &options.request_body_param_name)?;
    let request_default = if request_fields.iter().any(|field| field.required) {
        ""
    } else {
        " = {}"
    };
    let success = success_responses_of(op, graph)?;
    let data_ty = ts_success_data_type(&success);

    writeln!(
        out,
        "  async {raw_method_name}(requestParameters: {request_name}{request_default}, initOverrides: {} = {{}}): Promise<runtime.ApiResponse<{data_ty}>> {{",
        fetch_init_override_type(options)
    )
    .map_err(ts_mod_sink)?;
    emit_fetch_path(out, base_path, op)?;
    emit_fetch_query(out, op)?;
    writeln!(
        out,
        "    const headerParameters: runtime.HTTPHeaders = {{}};"
    )
    .map_err(ts_mod_sink)?;
    for (auth_index, (header, names)) in operation_auth_header_names(graph, op)?
        .into_iter()
        .enumerate()
    {
        let auth_var = format!("apiKey{auth_index}");
        writeln!(
            out,
            "    const {} = await this.configuration.getApiKey({});",
            auth_var,
            names
                .iter()
                .map(|name| quoted_string_literal(name))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "    if ({auth_var} !== undefined) {{").map_err(ts_mod_sink)?;
        writeln!(
            out,
            "      headerParameters[{}] = {};",
            quoted_string_literal(&header),
            auth_var
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "    }}").map_err(ts_mod_sink)?;
    }
    let body_model = request_body_model_of(op, graph)?;
    let multipart_fields = multipart_request_fields(graph, op)?;
    writeln!(out, "    const response = await this.request({{").map_err(ts_mod_sink)?;
    writeln!(out, "      path: localVarPath,").map_err(ts_mod_sink)?;
    writeln!(
        out,
        "      method: {},",
        quoted_string_literal(&op.method.to_uppercase())
    )
    .map_err(ts_mod_sink)?;
    writeln!(out, "      headers: headerParameters,").map_err(ts_mod_sink)?;
    writeln!(out, "      query: queryParameters,").map_err(ts_mod_sink)?;
    if let Some(fields) = &multipart_fields {
        let parts = fields
            .iter()
            .map(|field| {
                format!(
                    "{}: requestParameters.{}",
                    quoted_string_literal(&field.wire_name),
                    field.name
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "      body: multipartBody({{ {parts} }}),").map_err(ts_mod_sink)?;
    } else if let Some(body) = &body_model {
        if body.required {
            writeln!(
                out,
                "      body: requestParameters.{},",
                options.request_body_param_name
            )
            .map_err(ts_mod_sink)?;
        } else {
            writeln!(
                out,
                "      ...(requestParameters.{name} === undefined ? {{}} : {{ body: requestParameters.{name} }}),",
                name = options.request_body_param_name
            )
            .map_err(ts_mod_sink)?;
        }
    }
    writeln!(out, "    }}, initOverrides);").map_err(ts_mod_sink)?;
    writeln!(
        out,
        "    if (response.status < 200 || response.status >= 300) {{"
    )
    .map_err(ts_mod_sink)?;
    writeln!(out, "      throw new runtime.ResponseError(response);").map_err(ts_mod_sink)?;
    writeln!(out, "    }}").map_err(ts_mod_sink)?;
    emit_fetch_api_response(out, &success)?;
    writeln!(out, "  }}").map_err(ts_mod_sink)?;

    writeln!(
        out,
        "\n  async {method_name}(requestParameters: {request_name}{request_default}, initOverrides: {} = {{}}): Promise<{data_ty}> {{",
        fetch_init_override_type(options)
    )
    .map_err(ts_mod_sink)?;
    writeln!(
        out,
        "    const response = await this.{raw_method_name}(requestParameters, initOverrides);"
    )
    .map_err(ts_mod_sink)?;
    writeln!(out, "    return await response.value();").map_err(ts_mod_sink)?;
    writeln!(out, "  }}").map_err(ts_mod_sink)?;
    Ok(())
}

fn emit_fetch_api_response(
    out: &mut String,
    success: &crate::sdk::emit_common::SuccessResponses,
) -> Result<(), crate::CoreError> {
    if success.has_binary_body() {
        if success.has_bodyless_alternative() {
            writeln!(
                out,
                "    if (![{}].includes(response.status)) {{",
                success
                    .binary_statuses
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .map_err(ts_mod_sink)?;
            writeln!(
                out,
                "      return new runtime.VoidApiResponse(response) as runtime.ApiResponse<{data_ty}>;",
                data_ty = ts_success_data_type(success)
            )
            .map_err(ts_mod_sink)?;
            writeln!(out, "    }}").map_err(ts_mod_sink)?;
        }
        writeln!(out, "    return new runtime.BlobApiResponse(response);").map_err(ts_mod_sink)?;
        return Ok(());
    }
    if let Some(model) = &success.body_model {
        if success.has_bodyless_alternative() {
            writeln!(
                out,
                "    if (![{}].includes(response.status)) {{",
                success
                    .body_statuses
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .map_err(ts_mod_sink)?;
            writeln!(
                out,
                "      return new runtime.VoidApiResponse(response) as runtime.ApiResponse<{data_ty}>;",
                data_ty = ts_success_data_type(success)
            )
            .map_err(ts_mod_sink)?;
            writeln!(out, "    }}").map_err(ts_mod_sink)?;
        }
        writeln!(
            out,
            "    return new runtime.JSONApiResponse(response, (json) => json as models.{model});"
        )
        .map_err(ts_mod_sink)?;
        return Ok(());
    }
    writeln!(out, "    return new runtime.VoidApiResponse(response);").map_err(ts_mod_sink)?;
    Ok(())
}

fn emit_fetch_path(
    out: &mut String,
    base_path: &str,
    op: &Operation,
) -> Result<(), crate::CoreError> {
    let mut template = join_path(base_path, &op.path);
    let tokens = path_tokens(&template);
    let mut param_set: Vec<&str> = op
        .params
        .iter()
        .filter(|param| param.location == "path")
        .map(|param| param.name.as_str())
        .collect();
    param_set.sort_unstable();
    if !path_tokens_match(&tokens, &param_set) {
        return Err(crate::CoreError::SdkGen {
            message: format!(
                "operation '{}' path '{}' templated tokens {:?} do not match its path params {:?}",
                op.id, template, tokens, param_set
            ),
        });
    }
    for param in op.params.iter().filter(|param| param.location == "path") {
        let ident = emit::camel(&param.name);
        template = template.replace(
            &format!("{{{}}}", param.name),
            &format!("${{encodeURIComponent(String(requestParameters.{ident}))}}"),
        );
    }
    writeln!(out, "    const localVarPath = `{template}`;").map_err(ts_mod_sink)?;
    Ok(())
}

fn emit_fetch_query(out: &mut String, op: &Operation) -> Result<(), crate::CoreError> {
    writeln!(out, "    const queryParameters: runtime.HTTPQuery = {{}};").map_err(ts_mod_sink)?;
    for param in op.params.iter().filter(|param| param.location == "query") {
        let ident = emit::camel(&param.name);
        if param.required {
            writeln!(
                out,
                "    queryParameters[{}] = requestParameters.{ident};",
                quoted_string_literal(&param.name)
            )
            .map_err(ts_mod_sink)?;
        } else {
            writeln!(out, "    if (requestParameters.{ident} !== undefined) {{")
                .map_err(ts_mod_sink)?;
            writeln!(
                out,
                "      queryParameters[{}] = requestParameters.{ident};",
                quoted_string_literal(&param.name)
            )
            .map_err(ts_mod_sink)?;
            writeln!(out, "    }}").map_err(ts_mod_sink)?;
        }
    }
    Ok(())
}

fn operation_auth_header_names(
    graph: &ApiGraph,
    op: &Operation,
) -> Result<Vec<(String, Vec<String>)>, crate::CoreError> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for scheme in operation_api_key_schemes(graph, op)? {
        if scheme.location != ApiKeyLocation::Header {
            continue;
        }
        let names = grouped.entry(scheme.name.clone()).or_default();
        names.push(scheme.id);
        names.push(scheme.name);
    }
    for names in grouped.values_mut() {
        names.sort();
        names.dedup();
    }
    Ok(grouped.into_iter().collect())
}

fn emit_axios_configuration() -> String {
    "\
import type { AxiosRequestConfig } from \"axios\";

export type ApiKey = string | (() => string | Promise<string>);

export interface ConfigurationParameters {
  basePath?: string;
  apiKey?: ApiKey;
  apiKeys?: Record<string, ApiKey>;
  baseOptions?: AxiosRequestConfig;
}

export class Configuration {
  public readonly basePath: string;
  public readonly apiKey?: ApiKey;
  public readonly apiKeys: Record<string, ApiKey>;
  public readonly baseOptions?: AxiosRequestConfig;

  constructor(parameters: ConfigurationParameters = {}) {
    this.basePath = (parameters.basePath ?? \"\").replace(/\\/+$/, \"\");
    this.apiKey = parameters.apiKey;
    this.apiKeys = parameters.apiKeys ?? {};
    this.baseOptions = parameters.baseOptions;
  }

  async getApiKey(...names: string[]): Promise<string | undefined> {
    for (const name of names) {
      const value = this.apiKeys[name];
      if (typeof value === \"function\") {
        return await value();
      }
      if (value !== undefined) {
        return value;
      }
    }
    if (typeof this.apiKey === \"function\") {
      return await this.apiKey();
    }
    return this.apiKey;
  }
}
"
    .to_string()
}

fn emit_axios_base() -> String {
    "\
import axios, { AxiosInstance } from \"axios\";
import { Configuration } from \"./configuration\";

export const BASE_PATH = \"\";

export class BaseAPI {
  protected configuration: Configuration;
  protected axios: AxiosInstance;
  protected basePath: string;

  constructor(
    configuration: Configuration = new Configuration(),
    basePath: string = configuration.basePath,
    axiosInstance: AxiosInstance = axios,
  ) {
    this.configuration = configuration;
    this.basePath = basePath.replace(/\\/+$/, \"\");
    this.axios = axiosInstance;
  }
}
"
    .to_string()
}

fn emit_axios_index() -> String {
    "\
export * from \"./api\";
export * from \"./base\";
export * from \"./configuration\";
export * from \"./errors\";
export * from \"./models\";
"
    .to_string()
}

fn emit_axios_package_json(package: &str) -> String {
    format!(
        "{{
  \"name\": {},
  \"version\": \"0.1.0\",
  \"type\": \"module\",
  \"main\": \"./index.js\",
  \"types\": \"./index.d.ts\",
  \"dependencies\": {{
    \"axios\": \"^1.0.0\"
  }}
}}
",
        quoted_string_literal(package)
    )
}

fn emit_axios_api(
    graph: &ApiGraph,
    base_path: &str,
    response_policy: TsResponsePolicy,
) -> Result<String, crate::CoreError> {
    let mut out = String::new();
    let axios_types = match response_policy {
        TsResponsePolicy::DataOnly => "AxiosInstance, AxiosRequestConfig",
        TsResponsePolicy::AxiosResponseWrapper => {
            "AxiosInstance, AxiosRequestConfig, AxiosResponse"
        }
    };
    writeln!(out, "import type {{ {axios_types} }} from \"axios\";").map_err(ts_mod_sink)?;
    out.push_str(
        "\
import { BaseAPI } from \"./base\";
import { Configuration } from \"./configuration\";
import { ApiError } from \"./errors\";
import * as models from \"./models\";

",
    );

    let grouped = grouped_operations(graph);
    for (service, ops) in grouped {
        let class_name = api_class_name(&service);
        for op in &ops {
            emit_axios_request_alias(&mut out, graph, op)?;
        }
        writeln!(out, "export class {class_name} extends BaseAPI {{").map_err(ts_mod_sink)?;
        for op in &ops {
            emit_axios_operation_method(&mut out, graph, base_path, op, response_policy)?;
        }
        writeln!(out, "}}\n").map_err(ts_mod_sink)?;
        emit_axios_factory(&mut out, graph, &class_name, &ops, response_policy)?;
        for op in &ops {
            writeln!(
                out,
                "export type {}Operation = {class_name}[{}];",
                pascal(&op.handler),
                quoted_string_literal(&emit::camel(&op.handler))
            )
            .map_err(ts_mod_sink)?;
        }
        out.push('\n');
    }
    Ok(out)
}

fn grouped_operations(graph: &ApiGraph) -> BTreeMap<String, Vec<&Operation>> {
    let mut grouped: BTreeMap<String, Vec<&Operation>> = BTreeMap::new();
    for op in &graph.operations {
        grouped
            .entry(op.group.clone().unwrap_or_else(|| "default".to_string()))
            .or_default()
            .push(op);
    }
    grouped
}

fn emit_axios_request_alias(
    out: &mut String,
    graph: &ApiGraph,
    op: &Operation,
) -> Result<(), crate::CoreError> {
    emit_request_alias(out, graph, op, "body")
}

fn emit_request_alias(
    out: &mut String,
    graph: &ApiGraph,
    op: &Operation,
    request_body_param_name: &str,
) -> Result<(), crate::CoreError> {
    let request_name = request_alias_name(op);
    let fields = request_fields(graph, op, request_body_param_name)?;
    if fields.is_empty() {
        writeln!(out, "export interface {request_name} {{}}\n").map_err(ts_mod_sink)?;
        return Ok(());
    }
    writeln!(out, "export interface {request_name} {{").map_err(ts_mod_sink)?;
    for field in fields {
        let optional = if field.required { "" } else { "?" };
        writeln!(out, "  {}{optional}: {};", field.name, field.ty).map_err(ts_mod_sink)?;
    }
    writeln!(out, "}}\n").map_err(ts_mod_sink)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn emit_axios_operation_method(
    out: &mut String,
    graph: &ApiGraph,
    base_path: &str,
    op: &Operation,
    response_policy: TsResponsePolicy,
) -> Result<(), crate::CoreError> {
    let method_name = emit::camel(&op.handler);
    let request_name = request_alias_name(op);
    let request_fields = request_fields(graph, op, "body")?;
    let request_default = if request_fields.iter().any(|field| field.required) {
        ""
    } else {
        " = {}"
    };
    let success = success_responses_of(op, graph)?;
    let data_ty = ts_success_data_type(&success);
    let return_ty = axios_operation_return_type(&data_ty, response_policy);

    writeln!(
        out,
        "  async {method_name}(requestParameters: {request_name}{request_default}, options: AxiosRequestConfig = {{}}): {return_ty} {{"
    )
    .map_err(ts_mod_sink)?;
    emit_axios_path(out, base_path, op)?;
    emit_axios_query(out, op)?;
    writeln!(
        out,
        "    const localVarHeaderParameter: Record<string, string> = {{}};"
    )
    .map_err(ts_mod_sink)?;
    for (auth_index, (header, names)) in operation_auth_header_names(graph, op)?
        .into_iter()
        .enumerate()
    {
        let auth_var = format!("apiKey{auth_index}");
        writeln!(
            out,
            "    const {} = await this.configuration.getApiKey({});",
            auth_var,
            names
                .iter()
                .map(|name| quoted_string_literal(name))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "    if ({auth_var} !== undefined) {{").map_err(ts_mod_sink)?;
        writeln!(
            out,
            "      localVarHeaderParameter[{}] = {};",
            quoted_string_literal(&header),
            auth_var
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "    }}").map_err(ts_mod_sink)?;
    }
    let body_model = request_body_model_of(op, graph)?;
    if let Some(body_model) = &body_model {
        writeln!(
            out,
            "    const localVarRequestOptions: AxiosRequestConfig = {{"
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "      ...(this.configuration.baseOptions || {{}}),").map_err(ts_mod_sink)?;
        writeln!(out, "      ...options,").map_err(ts_mod_sink)?;
        writeln!(
            out,
            "      method: {},",
            quoted_string_literal(&op.method.to_uppercase())
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "      url: this.basePath + localVarPath,").map_err(ts_mod_sink)?;
        writeln!(out, "      params: localVarQueryParameter,").map_err(ts_mod_sink)?;
        if body_model.required {
            writeln!(out, "      data: requestParameters.body,").map_err(ts_mod_sink)?;
        } else {
            writeln!(
                out,
                "      ...(requestParameters.body === undefined ? {{}} : {{ data: requestParameters.body }}),"
            )
            .map_err(ts_mod_sink)?;
        }
    } else {
        writeln!(
            out,
            "    const localVarRequestOptions: AxiosRequestConfig = {{"
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "      ...(this.configuration.baseOptions || {{}}),").map_err(ts_mod_sink)?;
        writeln!(out, "      ...options,").map_err(ts_mod_sink)?;
        writeln!(
            out,
            "      method: {},",
            quoted_string_literal(&op.method.to_uppercase())
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "      url: this.basePath + localVarPath,").map_err(ts_mod_sink)?;
        writeln!(out, "      params: localVarQueryParameter,").map_err(ts_mod_sink)?;
    }
    writeln!(out, "      headers: {{").map_err(ts_mod_sink)?;
    writeln!(
        out,
        "        ...((this.configuration.baseOptions?.headers as Record<string, string> | undefined) || {{}}),"
    )
    .map_err(ts_mod_sink)?;
    writeln!(
        out,
        "        ...((options.headers as Record<string, string> | undefined) || {{}}),"
    )
    .map_err(ts_mod_sink)?;
    writeln!(out, "        ...localVarHeaderParameter,").map_err(ts_mod_sink)?;
    writeln!(out, "      }},").map_err(ts_mod_sink)?;
    writeln!(out, "      validateStatus: () => true,").map_err(ts_mod_sink)?;
    if success.has_binary_body() {
        writeln!(out, "      responseType: \"blob\",").map_err(ts_mod_sink)?;
    }
    writeln!(out, "    }};").map_err(ts_mod_sink)?;
    writeln!(
        out,
        "    const response = await this.axios.request(localVarRequestOptions);"
    )
    .map_err(ts_mod_sink)?;
    writeln!(
        out,
        "    if (response.status < 200 || response.status >= 300) {{"
    )
    .map_err(ts_mod_sink)?;
    writeln!(
        out,
        "      throw new ApiError(response.status, response.data);"
    )
    .map_err(ts_mod_sink)?;
    writeln!(out, "    }}").map_err(ts_mod_sink)?;
    if success.has_binary_body() {
        if response_policy == TsResponsePolicy::DataOnly && success.has_bodyless_alternative() {
            writeln!(
                out,
                "    if (![{}].includes(response.status)) {{",
                success
                    .binary_statuses
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .map_err(ts_mod_sink)?;
            writeln!(out, "      return undefined;").map_err(ts_mod_sink)?;
            writeln!(out, "    }}").map_err(ts_mod_sink)?;
        }
        match response_policy {
            TsResponsePolicy::DataOnly => {
                writeln!(out, "    return response.data as Blob;").map_err(ts_mod_sink)?;
            }
            TsResponsePolicy::AxiosResponseWrapper => {
                writeln!(out, "    return response as AxiosResponse<{data_ty}>;")
                    .map_err(ts_mod_sink)?;
            }
        }
    } else if let Some(model) = &success.body_model {
        if response_policy == TsResponsePolicy::DataOnly && success.has_bodyless_alternative() {
            writeln!(
                out,
                "    if (![{}].includes(response.status)) {{",
                success
                    .body_statuses
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .map_err(ts_mod_sink)?;
            writeln!(out, "      return undefined;").map_err(ts_mod_sink)?;
            writeln!(out, "    }}").map_err(ts_mod_sink)?;
        }
        match response_policy {
            TsResponsePolicy::DataOnly => {
                writeln!(out, "    return response.data as models.{model};")
                    .map_err(ts_mod_sink)?;
            }
            TsResponsePolicy::AxiosResponseWrapper => {
                writeln!(out, "    return response as AxiosResponse<{data_ty}>;")
                    .map_err(ts_mod_sink)?;
            }
        }
    } else {
        match response_policy {
            TsResponsePolicy::DataOnly => {
                writeln!(out, "    return;").map_err(ts_mod_sink)?;
            }
            TsResponsePolicy::AxiosResponseWrapper => {
                writeln!(out, "    return response as AxiosResponse<void>;")
                    .map_err(ts_mod_sink)?;
            }
        }
    }
    writeln!(out, "  }}").map_err(ts_mod_sink)?;
    Ok(())
}

fn emit_axios_factory(
    out: &mut String,
    graph: &ApiGraph,
    class_name: &str,
    ops: &[&Operation],
    response_policy: TsResponsePolicy,
) -> Result<(), crate::CoreError> {
    writeln!(
        out,
        "export const {class_name}Factory = function (configuration?: Configuration, basePath?: string, axiosInstance?: AxiosInstance) {{"
    )
    .map_err(ts_mod_sink)?;
    writeln!(
        out,
        "  const api = new {class_name}(configuration, basePath, axiosInstance);"
    )
    .map_err(ts_mod_sink)?;
    writeln!(out, "  return {{").map_err(ts_mod_sink)?;
    for op in ops {
        let method = emit::camel(&op.handler);
        let request_name = request_alias_name(op);
        let request_fields = request_fields(graph, op, "body")?;
        let request_default = if request_fields.iter().any(|field| field.required) {
            ""
        } else {
            " = {}"
        };
        let success = success_responses_of(op, graph)?;
        let data_ty = ts_success_data_type(&success);
        let return_ty = axios_operation_return_type(&data_ty, response_policy);
        writeln!(
            out,
            "    {method}(requestParameters: {request_name}{request_default}, options?: AxiosRequestConfig): {return_ty} {{"
        )
        .map_err(ts_mod_sink)?;
        writeln!(
            out,
            "      return api.{method}(requestParameters, options);"
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "    }},").map_err(ts_mod_sink)?;
    }
    writeln!(out, "  }};").map_err(ts_mod_sink)?;
    writeln!(out, "}};\n").map_err(ts_mod_sink)?;
    Ok(())
}

fn axios_operation_return_type(data_ty: &str, response_policy: TsResponsePolicy) -> String {
    match response_policy {
        TsResponsePolicy::DataOnly => format!("Promise<{data_ty}>"),
        TsResponsePolicy::AxiosResponseWrapper => format!("Promise<AxiosResponse<{data_ty}>>"),
    }
}

fn ts_success_data_type(success: &crate::sdk::emit_common::SuccessResponses) -> String {
    if success.has_binary_body() {
        if success.has_bodyless_alternative() {
            return "Blob | undefined".to_string();
        }
        return "Blob".to_string();
    }
    success.body_model.as_ref().map_or_else(
        || "void".to_string(),
        |model| {
            if success.has_bodyless_alternative() {
                format!("models.{model} | undefined")
            } else {
                format!("models.{model}")
            }
        },
    )
}

fn is_multipart_operation(op: &Operation) -> bool {
    op.request_body_content_type
        .as_deref()
        .is_some_and(|content_type| content_type.eq_ignore_ascii_case("multipart/form-data"))
}

fn multipart_request_fields(
    graph: &ApiGraph,
    op: &Operation,
) -> Result<Option<Vec<RequestField>>, crate::CoreError> {
    if !is_multipart_operation(op) {
        return Ok(None);
    }
    let Some(body) = &op.request_body else {
        return Ok(None);
    };
    let schema = graph
        .schemas
        .iter()
        .find(|schema| schema.id == body.ref_id)
        .ok_or_else(|| crate::CoreError::SdkGen {
            message: format!(
                "operation '{}' multipart request body references dangling $ref '{}'",
                op.id, body.ref_id
            ),
        })?;
    let Type::Object(fields) = &schema.body else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(fields.len());
    for field in fields {
        let name = emit::camel(&field.json_name);
        out.push(RequestField {
            name,
            wire_name: field.json_name.clone(),
            ty: multipart_field_type(field, graph)?,
            required: op.request_body_required && field.required,
        });
    }
    Ok(Some(out))
}

fn multipart_field_type(field: &Field, graph: &ApiGraph) -> Result<String, crate::CoreError> {
    if matches!(&field.schema, Type::Primitive(Prim::Bytes)) {
        return Ok(if field.nullable {
            "Blob | null".to_string()
        } else {
            "Blob".to_string()
        });
    }
    emit::ts_type(&field.schema, field.nullable, graph, "models.")
}

fn fetch_init_override_type(options: &TsSdkOptions) -> &'static str {
    if options.init_override_function {
        "RequestInit | runtime.InitOverrideFunction"
    } else {
        "RequestInit"
    }
}

fn emit_axios_path(
    out: &mut String,
    base_path: &str,
    op: &Operation,
) -> Result<(), crate::CoreError> {
    let mut template = join_path(base_path, &op.path);
    let tokens = path_tokens(&template);
    let mut param_set: Vec<&str> = op
        .params
        .iter()
        .filter(|param| param.location == "path")
        .map(|param| param.name.as_str())
        .collect();
    param_set.sort_unstable();
    if !path_tokens_match(&tokens, &param_set) {
        return Err(crate::CoreError::SdkGen {
            message: format!(
                "operation '{}' path '{}' templated tokens {:?} do not match its path params {:?}",
                op.id, template, tokens, param_set
            ),
        });
    }
    for param in op.params.iter().filter(|param| param.location == "path") {
        let ident = emit::camel(&param.name);
        template = template.replace(
            &format!("{{{}}}", param.name),
            &format!("${{encodeURIComponent(String(requestParameters.{ident}))}}"),
        );
    }
    writeln!(out, "    const localVarPath = `{template}`;").map_err(ts_mod_sink)?;
    Ok(())
}

fn emit_axios_query(out: &mut String, op: &Operation) -> Result<(), crate::CoreError> {
    writeln!(
        out,
        "    const localVarQueryParameter: Record<string, unknown> = {{}};"
    )
    .map_err(ts_mod_sink)?;
    for param in op.params.iter().filter(|param| param.location == "query") {
        let ident = emit::camel(&param.name);
        if param.required {
            writeln!(
                out,
                "    localVarQueryParameter[{}] = requestParameters.{ident};",
                quoted_string_literal(&param.name)
            )
            .map_err(ts_mod_sink)?;
        } else {
            writeln!(out, "    if (requestParameters.{ident} !== undefined) {{")
                .map_err(ts_mod_sink)?;
            writeln!(
                out,
                "      localVarQueryParameter[{}] = requestParameters.{ident};",
                quoted_string_literal(&param.name)
            )
            .map_err(ts_mod_sink)?;
            writeln!(out, "    }}").map_err(ts_mod_sink)?;
        }
    }
    Ok(())
}

struct RequestField {
    name: String,
    ty: String,
    required: bool,
    wire_name: String,
}

fn request_fields(
    graph: &ApiGraph,
    op: &Operation,
    request_body_param_name: &str,
) -> Result<Vec<RequestField>, crate::CoreError> {
    let mut fields = Vec::new();
    let body_model = request_body_model_of(op, graph)?;
    let multipart_fields = multipart_request_fields(graph, op)?;
    let mut used_names: Vec<String> = if body_model.is_some() && multipart_fields.is_none() {
        validate_request_body_param_name(request_body_param_name)?;
        vec![request_body_param_name.to_string()]
    } else {
        Vec::new()
    };
    for param in &op.params {
        let name = emit::camel(&param.name);
        if !is_ts_request_identifier(&name) {
            return Err(crate::CoreError::SdkGen {
                message: format!(
                    "operation '{}' has a parameter named '{}' that yields an invalid TypeScript \
                     request field identifier '{}'",
                    op.id, param.name, name
                ),
            });
        }
        if used_names.contains(&name) {
            return Err(crate::CoreError::SdkGen {
                message: format!(
                    "operation '{}' has a parameter whose TypeScript request field name '{name}' \
                     collides with another request field",
                    op.id
                ),
            });
        }
        used_names.push(name.clone());
        fields.push(RequestField {
            wire_name: param.name.clone(),
            name,
            ty: emit::ts_type(&param.schema, false, graph, "models.")?,
            required: param.required || param.location == "path",
        });
    }
    if let Some(multipart_fields) = multipart_fields {
        for field in multipart_fields {
            if !is_ts_request_identifier(&field.name) {
                return Err(crate::CoreError::SdkGen {
                    message: format!(
                        "operation '{}' has multipart field '{}' whose TypeScript request field \
                         identifier '{}' is invalid",
                        op.id, field.wire_name, field.name
                    ),
                });
            }
            if used_names.contains(&field.name) {
                return Err(crate::CoreError::SdkGen {
                    message: format!(
                        "operation '{}' has multipart field '{}' whose TypeScript request field name '{}' \
                         collides with another request field",
                        op.id, field.wire_name, field.name
                    ),
                });
            }
            used_names.push(field.name.clone());
            fields.push(field);
        }
    } else if let Some(model) = body_model {
        fields.push(RequestField {
            name: request_body_param_name.to_string(),
            wire_name: request_body_param_name.to_string(),
            ty: format!("models.{}", model.model),
            required: model.required,
        });
    }
    Ok(fields)
}

fn validate_request_body_param_name(name: &str) -> Result<(), crate::CoreError> {
    if is_ts_request_identifier(name) {
        return Ok(());
    }
    Err(crate::CoreError::Config {
        message: format!(
            "TsSdk request_body_param_name must be a valid TypeScript request field identifier, got '{name}'"
        ),
    })
}

fn is_ts_request_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn request_alias_name(op: &Operation) -> String {
    format!("{}Request", pascal(&op.handler))
}

fn api_class_name(service: &str) -> String {
    format!("{}Api", pascal(service))
}

fn pascal(value: &str) -> String {
    let mut out = String::new();
    for word in split_words(value) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(&chars.as_str().to_ascii_lowercase());
        }
    }
    if out.is_empty() {
        "Default".to_string()
    } else {
        out
    }
}

fn ts_mod_sink(err: std::fmt::Error) -> crate::CoreError {
    crate::CoreError::SdkGen {
        message: format!("failed to format TypeScript axios source: {err}"),
    }
}

/// Split a generated SDK bundle String into its `(file_name, contents)` pairs.
///
/// Wraps the crate-private [`bundle::parse`] framing so the lifecycle layer can enumerate the SDK's
/// per-file outputs without re-implementing the marker split. Single source of truth for the framing —
/// the same one [`write_to_dir`] uses. (Consumed by the `TsSdk` target in `sdk::builtins`.)
#[cfg(test)]
pub(crate) fn split_bundle(bundle: &str) -> Vec<(String, String)> {
    crate::sdk::bundle::parse(bundle)
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code. Unlike the Go twin, these tests
    // require NO toolchain — `generate` is pure string emission with no `tsc`/`node` subprocess (the
    // hermetic typecheck lands in plan 03's tests/tssdk_compile.rs).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        generate, generate_files_with_profile, generate_files_with_profile_options,
        generate_with_layout, split_bundle,
    };
    use crate::analyze::facts::FieldMeta;
    use crate::graph::{
        ApiGraph, Field, Param, Prim, Response, Schema, SchemaRef, SecurityScheme, SourceSpan, Type,
    };
    use crate::sdk::bundle::write_to_dir;
    use crate::sdk::layout::SdkFileLayout;
    use crate::sdk::profile::SdkProfile;
    use crate::sdk::surface::SdkTypeAliases;
    use crate::sdk::typescript::{
        TsBarrelExports, TsNullablePolicy, TsResponsePolicy, TsSdkOptions,
    };

    /// A facts document covering one body POST and one query GET plus the request/response models +
    /// a named enum — enough to assert the four-file bundle shape and determinism without a toolchain.
    const SAMPLE: &[u8] = br#"{
      "module": "app",
      "routes": [
        {
          "method": "POST", "path": "/books", "handler": "createBook",
          "operation_id": "createBook", "params": [],
          "request_body": { "ref_id": "app.models.Book" },
          "responses": [
            { "status": 201, "body": { "ref_id": "app.models.CreatedMessage" } }
          ],
          "span": { "file": "/root/main.ts", "start_line": 1, "end_line": 1 }
        },
        {
          "method": "GET", "path": "/list", "handler": "listBooks",
          "operation_id": "listBooks",
          "params": [
            { "name": "cursor", "location": "query", "required": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "span": { "file": "/root/main.ts", "start_line": 2, "end_line": 2 } }
          ],
          "request_body": null,
          "responses": [ { "status": 200, "body": { "ref_id": "app.models.Book" } } ],
          "span": { "file": "/root/main.ts", "start_line": 2, "end_line": 2 }
        }
      ],
      "schemas": [
        {
          "id": "app.models.Book", "name": "Book",
          "body": { "type": "object", "of": [
            { "json_name": "format", "required": false, "optional": true, "nullable": false,
              "schema": { "type": "named", "of": "app.models.BookFormat" },
              "description": null, "example": null },
            { "json_name": "title", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "string" } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.ts", "start_line": 1, "end_line": 1 }
        },
        {
          "id": "app.models.BookFormat", "name": "BookFormat",
          "body": { "type": "enum", "of": ["hardcover", "paperback"] },
          "span": { "file": "/root/m.ts", "start_line": 2, "end_line": 2 }
        },
        {
          "id": "app.models.CreatedMessage", "name": "CreatedMessage",
          "body": { "type": "object", "of": [
            { "json_name": "id", "required": true, "optional": false, "nullable": false,
              "schema": { "type": "primitive", "of": { "prim": "int", "bits": 64, "signed": true } },
              "description": null, "example": null }
          ] },
          "span": { "file": "/root/m.ts", "start_line": 3, "end_line": 3 }
        }
      ],
      "diagnostics": []
    }"#;

    fn sample_graph() -> ApiGraph {
        let facts = serde_json::from_slice(SAMPLE).unwrap();
        ApiGraph::from_facts(facts, "/root")
    }

    #[test]
    fn generate_returns_ok_with_the_four_file_markers_in_fixed_order() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        let order: Vec<usize> = [
            "// ==== gnr8:file client.ts ====",
            "// ==== gnr8:file errors.ts ====",
            "// ==== gnr8:file index.ts ====",
            "// ==== gnr8:file models.ts ====",
        ]
        .iter()
        .map(|m| out.find(m).unwrap_or_else(|| panic!("missing {m}:\n{out}")))
        .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "markers must appear in the fixed client/errors/index/models order:\n{out}"
        );
    }

    #[test]
    fn generate_is_byte_identical_across_two_runs() {
        let graph = sample_graph();
        assert_eq!(
            generate(&graph, "bookstore", "/").unwrap(),
            generate(&graph, "bookstore", "/").unwrap(),
            "two generate runs must be byte-identical"
        );
    }

    #[test]
    fn generated_client_contains_the_operation_methods_and_models_the_enum() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        assert!(out.contains("async createBook(body: models.Book)"), "{out}");
        assert!(out.contains("async listBooks(cursor?: string)"), "{out}");
        assert!(
            out.contains("export type BookFormat = \"hardcover\" | \"paperback\";"),
            "{out}"
        );
        assert!(out.contains("export interface Book {"), "{out}");
    }

    #[test]
    fn openapi_generator_compat_profile_emits_axios_surface() {
        let files = generate_files_with_profile(
            &sample_graph(),
            "bookstore",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::openapi_generator_compat(),
        )
        .unwrap();
        let names: Vec<&str> = files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "api.ts",
                "base.ts",
                "configuration.ts",
                "errors.ts",
                "index.ts",
                "models.ts",
                "package.json",
            ]
        );
        let api = files
            .iter()
            .find(|file| file.name == "api.ts")
            .unwrap()
            .contents
            .as_str();
        assert!(
            api.contains("export class DefaultApi extends BaseAPI"),
            "{api}"
        );
        assert!(api.contains("export const DefaultApiFactory"), "{api}");
        assert!(api.contains("export interface CreateBookRequest"), "{api}");
        assert!(api.contains("export type CreateBookOperation"), "{api}");
        assert!(
            api.contains(
                "import type { AxiosInstance, AxiosRequestConfig, AxiosResponse } from \"axios\";"
            ),
            "{api}"
        );
        assert!(
            api.contains("async listBooks(requestParameters: ListBooksRequest = {}, options: AxiosRequestConfig = {}): Promise<AxiosResponse<models.Book>>"),
            "{api}"
        );
        assert!(
            api.contains("listBooks(requestParameters: ListBooksRequest = {}, options?: AxiosRequestConfig): Promise<AxiosResponse<models.Book>>"),
            "{api}"
        );
        assert!(
            api.contains("return response as AxiosResponse<models.Book>;"),
            "{api}"
        );

        let models = files
            .iter()
            .find(|file| file.name == "models.ts")
            .unwrap()
            .contents
            .as_str();
        assert!(models.contains("export const BookFormat = {"), "{models}");
        assert!(
            models.contains("export type BookFormat = typeof BookFormat[keyof typeof BookFormat];"),
            "{models}"
        );
        assert!(models.contains("title: string;"), "{models}");
        assert!(models.contains("format?: BookFormat;"), "{models}");

        let package = files
            .iter()
            .find(|file| file.name == "package.json")
            .unwrap()
            .contents
            .as_str();
        assert!(package.contains("\"axios\": \"^1.0.0\""), "{package}");
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "the test constructs a complete binary route and verifies the generated compatibility surface"
    )]
    fn typescript_fetch_compat_profile_emits_runtime_raw_methods_binary_and_scoped_headers() {
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
        graph.operations[0].group = Some("coursework".to_string());
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
                    file: "/root/main.ts".to_string(),
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
                    file: "/root/main.ts".to_string(),
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
            "@example/school-sdk",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::typescript_fetch_compat(),
        )
        .unwrap();
        let names: Vec<&str> = files.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "apis/index.ts",
                "index.ts",
                "models/index.ts",
                "package.json",
                "runtime.ts",
            ]
        );

        let runtime = files
            .iter()
            .find(|file| file.name == "runtime.ts")
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "export class Configuration",
            "export interface Middleware",
            "export class BaseAPI",
            "export interface ApiResponse<T>",
            "export class JSONApiResponse<T>",
            "export class VoidApiResponse",
            "export class BlobApiResponse",
            "export class ResponseError extends Error",
            "export class FetchError extends Error",
            "apiKeys: { ...this.apiKeys }",
            "next.configuration = this.configuration.withMiddleware(...middleware);",
        ] {
            assert!(runtime.contains(snippet), "missing {snippet}:\n{runtime}");
        }

        let api = files
            .iter()
            .find(|file| file.name == "apis/index.ts")
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "export class CourseworkApi extends runtime.BaseAPI",
            "export interface GetCourseworkSubmissionAttachmentRequest",
            "async getCourseworkSubmissionAttachmentRaw(requestParameters: GetCourseworkSubmissionAttachmentRequest",
            "Promise<runtime.ApiResponse<Blob>>",
            "async getCourseworkSubmissionAttachment(requestParameters: GetCourseworkSubmissionAttachmentRequest",
            "Promise<Blob>",
            "return new runtime.BlobApiResponse(response);",
            "const apiKey0 = await this.configuration.getApiKey(\"CSRFAuth\", \"X-CSRF-Token\");",
            "const apiKey1 = await this.configuration.getApiKey(\"ActiveSchoolAuth\", \"X-Plint-School-Id\");",
            "this.configuration.getApiKey(\"CSRFAuth\", \"X-CSRF-Token\")",
            "this.configuration.getApiKey(\"ActiveSchoolAuth\", \"X-Plint-School-Id\")",
            "headerParameters[\"X-CSRF-Token\"]",
            "headerParameters[\"X-Plint-School-Id\"]",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }

        let list_books_start = api
            .find("async listBooksRaw")
            .expect("sample graph should still emit listBooksRaw");
        let list_books_end = api[list_books_start..]
            .find("  async listBooks(")
            .map_or(api.len(), |offset| list_books_start + offset);
        let list_books_raw = &api[list_books_start..list_books_end];
        assert!(!list_books_raw.contains("X-CSRF-Token"), "{list_books_raw}");
        assert!(
            !list_books_raw.contains("X-Plint-School-Id"),
            "{list_books_raw}"
        );
    }

    #[test]
    fn typescript_fetch_compat_options_emit_request_body_init_override_and_named_barrel() {
        let profile = SdkProfile::typescript_fetch_compat();
        let mut options = TsSdkOptions::for_profile(&profile);
        options.request_body_param_name = "request".to_string();
        options.init_override_function = true;
        options.barrel_exports = TsBarrelExports::OpenApiGeneratorCompat;

        let files = generate_files_with_profile_options(
            &sample_graph(),
            "@example/bookstore-sdk",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &profile,
            &options,
        )
        .unwrap();

        let runtime = files
            .iter()
            .find(|file| file.name == "runtime.ts")
            .unwrap()
            .contents
            .as_str();
        assert!(
            runtime.contains("export interface InitOverrideFunction"),
            "{runtime}"
        );
        assert!(
            runtime.contains("initOverrides: RequestInit | InitOverrideFunction = {}"),
            "{runtime}"
        );

        let api = files
            .iter()
            .find(|file| file.name == "apis/index.ts")
            .unwrap()
            .contents
            .as_str();
        assert!(api.contains("  request: models.Book;"), "{api}");
        assert!(!api.contains("  body: models.Book;"), "{api}");
        assert!(api.contains("body: requestParameters.request,"), "{api}");
        assert!(
            api.contains("initOverrides: RequestInit | runtime.InitOverrideFunction = {}"),
            "{api}"
        );

        let index = files
            .iter()
            .find(|file| file.name == "index.ts")
            .unwrap()
            .contents
            .as_str();
        assert!(
            index.contains("  DefaultApi,\n} from \"./apis\";"),
            "{index}"
        );
        assert!(!index.contains("export * from \"./apis\";"), "{index}");
    }

    #[test]
    fn typescript_fetch_compat_flattens_multipart_form_fields() {
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
                file: "/root/main.ts".to_string(),
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
                    json_name: "note".to_string(),
                    required: false,
                    optional: true,
                    nullable: true,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                },
            ]),
            enum_source_order: Vec::new(),
            provenance: SourceSpan {
                file: "/root/main.ts".to_string(),
                start_line: 1,
                end_line: 1,
            },
        });

        let files = generate_files_with_profile(
            &graph,
            "@example/school-sdk",
            "/v1",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::typescript_fetch_compat(),
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.name == "apis/index.ts")
            .unwrap()
            .contents
            .as_str();
        for snippet in [
            "function multipartBody(fields: Record<string, Blob | string | number | boolean | null | undefined>): FormData",
            "if (value !== undefined && value !== null)",
            "export interface ExecuteImportJobRequest {\n  jobId: string;\n  bundle: Blob;\n  sourceKey?: string;\n  sourceSystem?: string;\n  note?: string | null;\n}",
            "body: multipartBody({ \"bundle\": requestParameters.bundle, \"sourceKey\": requestParameters.sourceKey, \"sourceSystem\": requestParameters.sourceSystem, \"note\": requestParameters.note }),",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }
    }

    #[test]
    fn typescript_fetch_compat_preserves_literal_file_multipart_field_name() {
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
                file: "/root/main.ts".to_string(),
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
                file: "/root/main.ts".to_string(),
                start_line: 1,
                end_line: 1,
            },
        });

        let files = generate_files_with_profile(
            &graph,
            "@example/documents-sdk",
            "/v1",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::typescript_fetch_compat(),
        )
        .unwrap();
        let api = files
            .iter()
            .find(|file| file.name == "apis/index.ts")
            .unwrap()
            .contents
            .as_str();

        for snippet in [
            "export interface UploadDocumentRequest {\n  documentId: string;\n  file: Blob;\n}",
            "body: multipartBody({ \"file\": requestParameters.file }),",
        ] {
            assert!(api.contains(snippet), "missing {snippet}:\n{api}");
        }
    }

    #[test]
    fn typescript_fetch_compat_rejects_invalid_request_body_param_name() {
        let profile = SdkProfile::typescript_fetch_compat();
        let mut options = TsSdkOptions::for_profile(&profile);
        options.request_body_param_name = "request-body".to_string();

        let err = generate_files_with_profile_options(
            &sample_graph(),
            "@example/bookstore-sdk",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &profile,
            &options,
        )
        .unwrap_err();

        assert!(err.to_string().contains("request_body_param_name"), "{err}");
    }

    #[test]
    fn compatibility_auth_header_variables_are_collision_free() {
        let mut graph = sample_graph();
        graph.security = vec![
            SecurityScheme {
                id: "PrimaryAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: "X-API-Key".to_string(),
                global: false,
            },
            SecurityScheme {
                id: "SecondaryAuth".to_string(),
                kind: "apiKey".to_string(),
                location: "header".to_string(),
                name: "X_APIKey".to_string(),
                global: false,
            },
        ];
        graph.operations[0].security = vec!["PrimaryAuth".to_string(), "SecondaryAuth".to_string()];

        let fetch_files = generate_files_with_profile(
            &graph,
            "@example/bookstore-sdk",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::typescript_fetch_compat(),
        )
        .unwrap();
        let fetch_api = fetch_files
            .iter()
            .find(|file| file.name == "apis/index.ts")
            .unwrap()
            .contents
            .as_str();
        assert!(
            fetch_api.contains("const apiKey0 = await this.configuration.getApiKey"),
            "{fetch_api}"
        );
        assert!(
            fetch_api.contains("const apiKey1 = await this.configuration.getApiKey"),
            "{fetch_api}"
        );

        let axios_files = generate_files_with_profile(
            &graph,
            "@example/bookstore-sdk",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::openapi_generator_compat(),
        )
        .unwrap();
        let axios_api = axios_files
            .iter()
            .find(|file| file.name == "api.ts")
            .unwrap()
            .contents
            .as_str();
        assert!(
            axios_api.contains("const apiKey0 = await this.configuration.getApiKey"),
            "{axios_api}"
        );
        assert!(
            axios_api.contains("const apiKey1 = await this.configuration.getApiKey"),
            "{axios_api}"
        );
    }

    #[test]
    fn openapi_generator_compat_profile_can_omit_null_from_optional_model_properties() {
        let mut graph = sample_graph();
        let Type::Object(fields) = &mut graph.schemas[0].body else {
            panic!("sample Book schema must be an object");
        };
        let format = fields
            .iter_mut()
            .find(|field| field.json_name == "format")
            .expect("sample format field");
        format.nullable = true;

        let mut options = TsSdkOptions::for_profile(&SdkProfile::openapi_generator_compat());
        options.nullable = TsNullablePolicy::OmitNullFromOptionalProperties;
        options.response = TsResponsePolicy::DataOnly;
        let files = generate_files_with_profile_options(
            &graph,
            "bookstore",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::openapi_generator_compat(),
            &options,
        )
        .unwrap();
        let models = files
            .iter()
            .find(|file| file.name == "models.ts")
            .unwrap()
            .contents
            .as_str();

        assert!(models.contains("format?: BookFormat;"), "{models}");
    }

    #[test]
    fn openapi_generator_compat_profile_defaults_to_explicit_nullability() {
        let mut graph = sample_graph();
        let Type::Object(fields) = &mut graph.schemas[0].body else {
            panic!("sample Book schema must be an object");
        };
        let title = fields
            .iter_mut()
            .find(|field| field.json_name == "title")
            .expect("sample title field");
        title.nullable = true;

        let files = generate_files_with_profile(
            &graph,
            "bookstore",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::openapi_generator_compat(),
        )
        .unwrap();
        let models = files
            .iter()
            .find(|file| file.name == "models.ts")
            .unwrap()
            .contents
            .as_str();

        assert!(models.contains("title: string | null;"), "{models}");
    }

    #[test]
    fn openapi_generator_compat_profile_rejects_path_param_mismatches() {
        let mut graph = sample_graph();
        graph.operations[0].path = "/books/{bookId}".to_string();

        let err = generate_files_with_profile(
            &graph,
            "bookstore",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::openapi_generator_compat(),
        )
        .unwrap_err();

        assert!(err.to_string().contains("templated tokens"), "{err}");
    }

    #[test]
    fn openapi_generator_compat_profile_rejects_request_field_collisions() {
        let mut graph = sample_graph();
        graph.operations[0].params.push(Param {
            name: "body".to_string(),
            location: "query".to_string(),
            required: false,
            schema: Type::Primitive(Prim::String),
            default: None,
            provenance: SourceSpan {
                file: "/root/main.ts".to_string(),
                start_line: 1,
                end_line: 1,
            },
        });

        let err = generate_files_with_profile(
            &graph,
            "bookstore",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::openapi_generator_compat(),
        )
        .unwrap_err();

        assert!(err.to_string().contains("collides"), "{err}");
    }

    #[test]
    fn split_bundle_round_trips_to_the_four_files() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["client.ts", "errors.ts", "index.ts", "models.ts"]
        );
        // The marker line must never appear inside a materialized file's contents.
        for (_, contents) in &files {
            assert!(
                !contents.contains("// ==== gnr8:file"),
                "marker leaked into a file"
            );
        }
    }

    #[test]
    fn write_to_dir_rejects_an_unsafe_frame_name() {
        // A hand-forged bundle whose frame name escapes the target dir must be refused (T-05-01-01).
        let evil = "// ==== gnr8:file ../escape.ts ====\nexport const x = 1;\n";
        let dir = std::env::temp_dir();
        let err = write_to_dir(evil, &dir).unwrap_err();
        assert!(
            err.to_string().contains("unsafe name"),
            "unsafe frame name must be rejected: {err}"
        );
    }

    #[test]
    fn write_to_dir_materializes_the_four_files() {
        let out = generate(&sample_graph(), "bookstore", "/").unwrap();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("gnr8-tssdk-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_to_dir(&out, &dir).unwrap();
        for name in ["client.ts", "errors.ts", "index.ts", "models.ts"] {
            assert!(dir.join(name).is_file(), "missing materialized {name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_layout_emits_models_directory_with_barrel_file() {
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", &SdkFileLayout::split())
            .unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "api_default.ts",
                "client.ts",
                "errors.ts",
                "index.ts",
                "models/book.ts",
                "models/book_format.ts",
                "models/created_message.ts",
                "models/index.ts",
            ]
        );
        assert!(
            !names.contains(&"models.ts"),
            "split layout must not emit compact models.ts"
        );
        assert!(
            out.contains("import { createBook as operation0_0 } from \"./api_default\";"),
            "client.ts should import split operation functions:\n{out}"
        );
        assert!(
            out.contains("Client.prototype.createBook = operation0_0;"),
            "client.ts should attach split operation functions to Client:\n{out}"
        );

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("gnr8-tssdk-split-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_to_dir(&out, &dir).unwrap();
        assert!(dir.join("api_default.ts").is_file());
        assert!(dir.join("models/book.ts").is_file());
        assert!(dir.join("models/index.ts").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_layout_can_emit_one_operation_file_per_endpoint() {
        let layout = SdkFileLayout::split().operations_per_endpoint();
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", &layout).unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"api_create_book.ts"), "{names:?}");
        assert!(names.contains(&"api_list_books.ts"), "{names:?}");
    }

    #[test]
    fn split_layout_preserves_group_facades() {
        let mut graph = sample_graph();
        for op in &mut graph.operations {
            op.group = Some("Books".to_string());
        }
        let out = generate_with_layout(&graph, "bookstore", "/", &SdkFileLayout::split()).unwrap();
        assert!(
            out.contains("get books(): BooksApi"),
            "split client should keep grouped facade getters:\n{out}"
        );
        assert!(
            out.contains("export class BooksApi"),
            "split client should keep grouped facade classes:\n{out}"
        );
    }

    #[test]
    fn split_operation_template_rejects_duplicate_rendered_files() {
        let layout = SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_file_template("api_{service_snake}.ts");
        let err = generate_with_layout(&sample_graph(), "bookstore", "/", &layout).unwrap_err();
        assert!(
            err.to_string().contains("duplicate SDK file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn split_operation_template_rejects_non_ts_files() {
        let layout = SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_file_template("api_{operation_snake}.js");
        let err = generate_with_layout(&sample_graph(), "bookstore", "/", &layout).unwrap_err();
        assert!(
            err.to_string().contains("must end with .ts"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn split_model_template_rejects_non_ts_files() {
        let layout = SdkFileLayout::split().model_file_template("models/{schema_snake}.js");
        let err = generate_with_layout(&sample_graph(), "bookstore", "/", &layout).unwrap_err();
        assert!(
            err.to_string().contains("must end with .ts"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn split_operation_import_escapes_custom_module_specifiers() {
        let mut graph = sample_graph();
        graph.operations[0].id = "create\"Book".to_string();
        let layout = SdkFileLayout::split()
            .operations_per_endpoint()
            .operation_file_template("api_{operation}.ts");
        let out = generate_with_layout(&graph, "bookstore", "/", &layout).unwrap();
        assert!(
            out.contains("from \"./api_create\\\"Book\";"),
            "client.ts should escape generated module specifiers:\n{out}"
        );
    }

    #[test]
    fn split_layout_can_place_models_in_a_configured_directory() {
        let layout = SdkFileLayout::split().model_dir("schemas");
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", &layout).unwrap();
        let files = split_bundle(&out);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"schemas/book.ts"));
        assert!(names.contains(&"schemas/index.ts"));
        assert!(
            out.contains("import * as models from \"./schemas\";"),
            "{out}"
        );
        assert!(out.contains("} from \"./schemas\";"), "{out}");
    }

    #[test]
    fn split_model_template_updates_model_barrel_paths() {
        let layout = SdkFileLayout::split()
            .model_dir("schemas")
            .model_file_template("types/{schema_kebab}.ts");
        let out = generate_with_layout(&sample_graph(), "bookstore", "/", &layout).unwrap();
        assert!(
            out.contains("export * from \"../types/book\";"),
            "schemas/index.ts should export actual rendered model paths:\n{out}"
        );
        assert!(
            out.contains("export * from \"../types/created-message\";"),
            "schemas/index.ts should preserve kebab model file names:\n{out}"
        );
        assert!(
            out.contains("import type * as models from \"../schemas/index\";"),
            "custom model files should import the configured model barrel:\n{out}"
        );
    }
}
