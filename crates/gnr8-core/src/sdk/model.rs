//! Deterministic SDK planning model.
//!
//! `SdkModel` is built from the graph before rendering. It records the public SDK surface and file
//! policy a target is about to emit while preserving the graph as the single source of source facts.

use std::collections::BTreeMap;

use crate::graph::{ApiGraph, Type};
use crate::sdk::emit_common::api_key_header_name;
use crate::sdk::layout::SdkFileLayout;
use crate::sdk::profile::SdkProfile;
use crate::sdk::surface::SdkTypeAliases;
use crate::CoreError;

/// SDK planning model derived from an [`ApiGraph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkModel {
    /// Package/module name supplied to the SDK target.
    pub package: String,
    /// API base path supplied by graph metadata.
    pub base_path: String,
    /// Optional auth metadata.
    pub auth: Option<SdkAuth>,
    /// Service/group plan.
    pub services: Vec<SdkService>,
    /// Operation surface.
    pub operations: Vec<SdkOperation>,
    /// Schema/model surface.
    pub schemas: Vec<SdkSchema>,
    /// Additional compatibility aliases.
    pub aliases: Vec<SdkAlias>,
    /// File layout plan.
    pub file_plan: SdkFilePlan,
    /// Compatibility metadata.
    pub compatibility: SdkCompatibility,
}

/// Auth metadata used by generated clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkAuth {
    /// Header name used for API-key auth.
    pub api_key_header: String,
}

/// A generated service/group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkService {
    /// Service name (`default` when no grouping transform assigned one).
    pub name: String,
    /// Operation ids in this service.
    pub operations: Vec<String>,
}

/// A generated operation method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkOperation {
    /// Stable operation id.
    pub id: String,
    /// Handler-derived method name source.
    pub handler: String,
    /// HTTP method.
    pub method: String,
    /// Group-relative path.
    pub path: String,
    /// Service/group name.
    pub service: String,
    /// Request schema name, when present.
    pub request_schema: Option<String>,
    /// Response body schema names keyed by status.
    pub response_schemas: Vec<(u16, String)>,
}

/// A generated schema/model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkSchema {
    /// Stable graph schema id.
    pub id: String,
    /// Generated symbol name.
    pub name: String,
    /// Shape category.
    pub kind: SdkSchemaKind,
}

/// Schema category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkSchemaKind {
    /// Object/interface/model.
    Object,
    /// String enum/literal vocabulary.
    Enum,
    /// Scalar alias.
    Primitive,
    /// Well-known scalar alias.
    WellKnown,
    /// Array alias.
    Array,
    /// Map alias.
    Map,
    /// Reference alias.
    Reference,
    /// Union alias.
    Union,
    /// Free-form alias.
    Any,
}

/// Compatibility alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkAlias {
    /// Current canonical symbol.
    pub canonical: String,
    /// Additional public symbol.
    pub alias: String,
}

/// File layout plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkFilePlan {
    /// Whether split layout is used.
    pub split: bool,
    /// Operation directory.
    pub operation_dir: Option<String>,
    /// Model directory.
    pub model_dir: Option<String>,
    /// Operation file template.
    pub operation_file_template: Option<String>,
    /// Model file template.
    pub model_file_template: Option<String>,
}

/// Compatibility metadata for this model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkCompatibility {
    /// Profile name.
    pub profile: String,
    /// Whether OpenAPI-generator compatibility was requested.
    pub openapi_generator_compat: bool,
}

impl SdkModel {
    /// Build a deterministic SDK planning model.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Config`] for invalid alias configuration.
    pub fn build(
        graph: &ApiGraph,
        package: impl Into<String>,
        base_path: impl Into<String>,
        layout: &SdkFileLayout,
        aliases: &SdkTypeAliases,
        profile: &SdkProfile,
    ) -> Result<Self, CoreError> {
        let package = package.into();
        let base_path = base_path.into();
        let auth = api_key_header_name(graph)?.map(|api_key_header| SdkAuth { api_key_header });
        let resolved_aliases = aliases
            .resolve(graph)?
            .into_iter()
            .map(|alias| SdkAlias {
                canonical: alias.canonical,
                alias: alias.alias,
            })
            .collect();

        let mut operations = Vec::with_capacity(graph.operations.len());
        let mut services: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for op in &graph.operations {
            let service = op.group.clone().unwrap_or_else(|| "default".to_string());
            services
                .entry(service.clone())
                .or_default()
                .push(op.id.clone());
            let request_schema = op
                .request_body
                .as_ref()
                .map(|body| schema_name_by_ref(graph, &body.ref_id))
                .transpose()?;
            let response_schemas = op
                .responses
                .iter()
                .filter_map(|response| {
                    response.body.as_ref().map(|body| {
                        schema_name_by_ref(graph, &body.ref_id).map(|name| (response.status, name))
                    })
                })
                .collect::<Result<Vec<_>, CoreError>>()?;
            operations.push(SdkOperation {
                id: op.id.clone(),
                handler: op.handler.clone(),
                method: op.method.clone(),
                path: op.path.clone(),
                service,
                request_schema,
                response_schemas,
            });
        }

        Ok(Self {
            package,
            base_path,
            auth,
            services: services
                .into_iter()
                .map(|(name, operations)| SdkService { name, operations })
                .collect(),
            operations,
            schemas: graph
                .schemas
                .iter()
                .map(|schema| SdkSchema {
                    id: schema.id.clone(),
                    name: schema.name.clone(),
                    kind: schema_kind(&schema.body),
                })
                .collect(),
            aliases: resolved_aliases,
            file_plan: SdkFilePlan {
                split: layout.is_split(),
                operation_dir: layout.operation_dir_ref().map(ToString::to_string),
                model_dir: layout.model_dir_ref().map(ToString::to_string),
                operation_file_template: layout
                    .operation_file_template_ref()
                    .map(ToString::to_string),
                model_file_template: layout.model_file_template_ref().map(ToString::to_string),
            },
            compatibility: SdkCompatibility {
                profile: profile.name().to_string(),
                openapi_generator_compat: !profile.is_minimal(),
            },
        })
    }
}

fn schema_name_by_ref(graph: &ApiGraph, ref_id: &str) -> Result<String, CoreError> {
    graph
        .schemas
        .iter()
        .find(|schema| schema.id == ref_id)
        .map(|schema| schema.name.clone())
        .ok_or_else(|| CoreError::SdkGen {
            message: format!("dangling $ref '{ref_id}' is not among graph.schemas"),
        })
}

fn schema_kind(ty: &Type) -> SdkSchemaKind {
    match ty {
        Type::Object(_) => SdkSchemaKind::Object,
        Type::Enum(_) => SdkSchemaKind::Enum,
        Type::Primitive(_) => SdkSchemaKind::Primitive,
        Type::WellKnown(_) => SdkSchemaKind::WellKnown,
        Type::Array(_) => SdkSchemaKind::Array,
        Type::Map { .. } => SdkSchemaKind::Map,
        Type::Named(_) => SdkSchemaKind::Reference,
        Type::Union(_) => SdkSchemaKind::Union,
        Type::Any {} => SdkSchemaKind::Any,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{SdkModel, SdkSchemaKind};
    use crate::analyze::facts::FieldMeta;
    use crate::graph::{
        ApiGraph, Field, Operation, Prim, Response, Schema, SchemaRef, SourceSpan, Type,
    };
    use crate::sdk::layout::SdkFileLayout;
    use crate::sdk::profile::SdkProfile;
    use crate::sdk::surface::SdkTypeAliases;

    fn span() -> SourceSpan {
        SourceSpan {
            file: "api.go".to_string(),
            start_line: 1,
            end_line: 1,
        }
    }

    fn graph() -> ApiGraph {
        ApiGraph {
            operations: vec![Operation {
                id: "createBook".to_string(),
                method: "POST".to_string(),
                path: "/books".to_string(),
                handler: "createBook".to_string(),
                group: Some("Books".to_string()),
                params: vec![],
                request_body: Some(SchemaRef {
                    ref_id: "app.Book".to_string(),
                }),
                request_body_content_type: None,
                responses: vec![Response {
                    status: 201,
                    body: Some(SchemaRef {
                        ref_id: "app.Book".to_string(),
                    }),
                }],
                provenance: span(),
            }],
            schemas: vec![Schema {
                id: "app.Book".to_string(),
                name: "Book".to_string(),
                body: Type::Object(vec![Field {
                    json_name: "title".to_string(),
                    required: true,
                    optional: false,
                    nullable: false,
                    schema: Type::Primitive(Prim::String),
                    description: None,
                    example: None,
                    meta: FieldMeta::default(),
                }]),
                enum_source_order: Vec::new(),
                provenance: span(),
            }],
            ..ApiGraph::default()
        }
    }

    #[test]
    fn sdk_model_is_deterministic_and_carries_compatibility_metadata() {
        let aliases = SdkTypeAliases::new().type_alias("Book", "BookPayload");
        let layout = SdkFileLayout::split().model_dir("models");
        let profile = SdkProfile::openapi_generator_compat();

        let a = SdkModel::build(&graph(), "books", "/api", &layout, &aliases, &profile).unwrap();
        let b = SdkModel::build(&graph(), "books", "/api", &layout, &aliases, &profile).unwrap();

        assert_eq!(a, b);
        assert_eq!(a.services[0].name, "Books");
        assert_eq!(a.operations[0].request_schema.as_deref(), Some("Book"));
        assert_eq!(a.schemas[0].kind, SdkSchemaKind::Object);
        assert_eq!(a.aliases[0].alias, "BookPayload");
        assert!(a.compatibility.openapi_generator_compat);
        assert_eq!(a.file_plan.model_dir.as_deref(), Some("models"));
    }

    #[test]
    fn sdk_model_rejects_dangling_operation_refs() {
        let mut graph = graph();
        graph.operations[0].request_body = Some(SchemaRef {
            ref_id: "app.Missing".to_string(),
        });

        let err = SdkModel::build(
            &graph,
            "books",
            "/api",
            &SdkFileLayout::compact(),
            &SdkTypeAliases::default(),
            &SdkProfile::minimal(),
        )
        .unwrap_err();

        assert!(err.to_string().contains("app.Missing"), "{err}");
    }
}
