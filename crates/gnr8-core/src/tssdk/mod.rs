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

use crate::graph::{ApiGraph, Operation};
use crate::sdk::bundle::{SdkBundle, SdkFile};
use crate::sdk::emit_common::{
    api_key_header_name, body_model_of, check_unique_schema_names, file_in_dir, file_stem,
    join_path, model_file_name, path_tokens, path_tokens_match, quoted_string_literal, split_words,
    success_responses_of, validate_sdk_base_path,
};
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::profile::SdkProfile;
use crate::sdk::surface::SdkTypeAliases;
use crate::sdk::typescript::{TsResponsePolicy, TsSdkOptions};

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
    generate_files_with_layout_options(
        graph,
        package,
        base_path,
        layout,
        aliases,
        TsSdkOptions::strict(),
    )
}

fn generate_files_with_layout_options(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    options: TsSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    validate_sdk_base_path(base_path)?;
    check_unique_schema_names(graph, "TypeScript SDK")?;

    let mut files: Vec<SdkFile> = Vec::new();
    let auth_header = api_key_header_name(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;

    // Fixed alpha push order: client.ts, errors.ts, index.ts, models.ts — the D-06 frame order the
    // bundle locks. client.ts is the client skeleton followed by the operation methods.
    let ops: Vec<&Operation> = graph.operations.iter().collect();
    let model_dir = layout.model_dir_ref().unwrap_or("models");
    let mut client =
        emit::emit_client_with_models(package, model_dir.trim_matches('/'), auth_header.as_deref());
    client.push_str(&emit::emit_operations(
        graph,
        package,
        base_path,
        &ops,
        auth_header.as_deref(),
    )?);
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

    if layout.is_split() {
        files.push(SdkFile {
            name: file_in_dir(Some(model_dir), "index.ts"),
            contents: emit::emit_models_index(graph, &resolved_aliases)?,
        });
        for schema in &graph.schemas {
            let default_name =
                file_in_dir(Some(model_dir), &format!("{}.ts", file_stem(&schema.name)));
            let name = if layout.model_file_template_ref().is_some() {
                model_file_name(layout, schema, &format!("{}.ts", file_stem(&schema.name)))?
            } else {
                default_name
            };
            files.push(SdkFile {
                name,
                contents: emit::emit_model_schema_with_policies(
                    graph,
                    schema,
                    options.model_properties,
                    options.nullable,
                )?,
            });
        }
        for alias in &resolved_aliases {
            files.push(SdkFile {
                name: file_in_dir(Some(model_dir), &format!("{}.ts", file_stem(&alias.alias))),
                contents: emit::emit_model_alias(alias),
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

    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
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
    generate_files_with_profile_options(
        graph,
        package,
        base_path,
        layout,
        aliases,
        profile,
        TsSdkOptions::for_profile(profile),
    )
}

pub(crate) fn generate_files_with_profile_options(
    graph: &ApiGraph,
    package: &str,
    base_path: &str,
    layout: &SdkFileLayout,
    aliases: &SdkTypeAliases,
    profile: &SdkProfile,
    options: TsSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    if !profile.is_openapi_generator_compat() && options.response.is_axios_response_wrapper() {
        return Err(crate::CoreError::Config {
            message: "TsResponsePolicy::AxiosResponseWrapper requires SdkProfile::openapi_generator_compat()"
                .to_string(),
        });
    }
    if profile.is_minimal() {
        return generate_files_with_layout_options(
            graph, package, base_path, layout, aliases, options,
        );
    }
    if profile.is_openapi_generator_compat() {
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
    options: TsSdkOptions,
) -> Result<Vec<SdkFile>, crate::CoreError> {
    validate_sdk_base_path(base_path)?;
    check_unique_schema_names(graph, "TypeScript SDK")?;

    let auth_header = api_key_header_name(graph)?;
    let resolved_aliases = aliases.resolve(graph)?;
    let mut files = vec![
        SdkFile {
            name: "api.ts".to_string(),
            contents: emit_axios_api(graph, base_path, auth_header.as_deref(), options.response)?,
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
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn emit_axios_configuration() -> String {
    "\
import type { AxiosRequestConfig } from \"axios\";

export type ApiKey = string | (() => string | Promise<string>);

export interface ConfigurationParameters {
  basePath?: string;
  apiKey?: ApiKey;
  baseOptions?: AxiosRequestConfig;
}

export class Configuration {
  public readonly basePath: string;
  public readonly apiKey?: ApiKey;
  public readonly baseOptions?: AxiosRequestConfig;

  constructor(parameters: ConfigurationParameters = {}) {
    this.basePath = (parameters.basePath ?? \"\").replace(/\\/+$/, \"\");
    this.apiKey = parameters.apiKey;
    this.baseOptions = parameters.baseOptions;
  }

  async getApiKey(): Promise<string | undefined> {
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
    auth_header: Option<&str>,
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
            emit_axios_operation_method(
                &mut out,
                graph,
                base_path,
                op,
                auth_header,
                response_policy,
            )?;
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
    let request_name = request_alias_name(op);
    let fields = request_fields(graph, op)?;
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
    auth_header: Option<&str>,
    response_policy: TsResponsePolicy,
) -> Result<(), crate::CoreError> {
    let method_name = emit::camel(&op.handler);
    let request_name = request_alias_name(op);
    let request_fields = request_fields(graph, op)?;
    let request_default = if request_fields.iter().any(|field| field.required) {
        ""
    } else {
        " = {}"
    };
    let success = success_responses_of(op, graph)?;
    let data_ty = success.body_model.as_ref().map_or_else(
        || "void".to_string(),
        |model| {
            if success.has_bodyless_alternative() {
                format!("models.{model} | undefined")
            } else {
                format!("models.{model}")
            }
        },
    );
    let return_ty = axios_operation_return_type(&data_ty, response_policy);

    writeln!(
        out,
        "  async {method_name}(requestParameters: {request_name}{request_default}, options: AxiosRequestConfig = {{}}): {return_ty} {{"
    )
    .map_err(ts_mod_sink)?;
    emit_axios_path(out, base_path, op)?;
    emit_axios_query(out, op)?;
    if let Some(header) = auth_header {
        writeln!(
            out,
            "    const localVarHeaderParameter: Record<string, string> = {{}};"
        )
        .map_err(ts_mod_sink)?;
        writeln!(
            out,
            "    const apiKey = await this.configuration.getApiKey();"
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "    if (apiKey !== undefined) {{").map_err(ts_mod_sink)?;
        writeln!(
            out,
            "      localVarHeaderParameter[{}] = apiKey;",
            quoted_string_literal(header)
        )
        .map_err(ts_mod_sink)?;
        writeln!(out, "    }}").map_err(ts_mod_sink)?;
    } else {
        writeln!(
            out,
            "    const localVarHeaderParameter: Record<string, string> = {{}};"
        )
        .map_err(ts_mod_sink)?;
    }
    let body_model = body_model_of(op, graph)?;
    if body_model.is_some() {
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
        writeln!(out, "      data: requestParameters.body,").map_err(ts_mod_sink)?;
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
    if let Some(model) = &success.body_model {
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
        let request_fields = request_fields(graph, op)?;
        let request_default = if request_fields.iter().any(|field| field.required) {
            ""
        } else {
            " = {}"
        };
        let success = success_responses_of(op, graph)?;
        let data_ty = success.body_model.as_ref().map_or_else(
            || "void".to_string(),
            |model| {
                if success.has_bodyless_alternative() {
                    format!("models.{model} | undefined")
                } else {
                    format!("models.{model}")
                }
            },
        );
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
}

fn request_fields(graph: &ApiGraph, op: &Operation) -> Result<Vec<RequestField>, crate::CoreError> {
    let mut fields = Vec::new();
    let body_model = body_model_of(op, graph)?;
    let mut used_names: Vec<String> = if body_model.is_some() {
        vec!["body".to_string()]
    } else {
        Vec::new()
    };
    for param in &op.params {
        let name = emit::camel(&param.name);
        if name.is_empty() {
            return Err(crate::CoreError::SdkGen {
                message: format!(
                    "operation '{}' has a parameter named '{}' that yields an empty TypeScript \
                     request field name",
                    op.id, param.name
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
            name,
            ty: emit::ts_type(&param.schema, false, graph, "models.")?,
            required: param.required || param.location == "path",
        });
    }
    if let Some(model) = body_model {
        fields.push(RequestField {
            name: "body".to_string(),
            ty: format!("models.{model}"),
            required: true,
        });
    }
    Ok(fields)
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
    use crate::graph::{ApiGraph, Param, Prim, SourceSpan, Type};
    use crate::sdk::bundle::write_to_dir;
    use crate::sdk::layout::SdkFileLayout;
    use crate::sdk::profile::SdkProfile;
    use crate::sdk::surface::SdkTypeAliases;
    use crate::sdk::typescript::{TsNullablePolicy, TsResponsePolicy, TsSdkOptions};

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
        assert!(models.contains("title?: string;"), "{models}");

        let package = files
            .iter()
            .find(|file| file.name == "package.json")
            .unwrap()
            .contents
            .as_str();
        assert!(package.contains("\"axios\": \"^1.0.0\""), "{package}");
    }

    #[test]
    fn openapi_generator_compat_profile_can_omit_null_from_optional_model_properties() {
        let mut graph = sample_graph();
        let Type::Object(fields) = &mut graph.schemas[0].body else {
            panic!("sample Book schema must be an object");
        };
        let title = fields
            .iter_mut()
            .find(|field| field.json_name == "title")
            .expect("sample title field");
        title.nullable = true;

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
            options,
        )
        .unwrap();
        let models = files
            .iter()
            .find(|file| file.name == "models.ts")
            .unwrap()
            .contents
            .as_str();

        assert!(models.contains("title?: string;"), "{models}");
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

        assert!(models.contains("title?: string | null;"), "{models}");
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

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir =
            std::env::temp_dir().join(format!("gnr8-tssdk-split-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_to_dir(&out, &dir).unwrap();
        assert!(dir.join("models/book.ts").is_file());
        assert!(dir.join("models/index.ts").is_file());
        let _ = std::fs::remove_dir_all(&dir);
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
}
