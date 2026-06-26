//! SDK compatibility-surface configuration shared by built-in language targets.
//!
//! These types describe public SDK shape, not source facts. A target maps them into language-native
//! aliases/shims while the graph remains the single source of truth for operations and schemas.

use crate::graph::ApiGraph;
use crate::CoreError;

/// A schema symbol compatibility alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedTypeAlias {
    /// The current generated schema symbol.
    pub(crate) canonical: String,
    /// The additional compatibility symbol to expose.
    pub(crate) alias: String,
}

/// Additional SDK type names to expose for compatibility with an existing public SDK surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SdkTypeAliases {
    schema_aliases: Vec<(String, String)>,
    legacy_source_prefixes: bool,
}

impl SdkTypeAliases {
    /// No compatibility aliases.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Expose `alias` as an additional name for the schema matched by `schema`.
    ///
    /// `schema` may be either the graph schema id or its generated bare name. The alias does not
    /// rename the canonical schema; it adds a compatibility surface next to it.
    #[must_use]
    pub fn type_alias(mut self, schema: impl Into<String>, alias: impl Into<String>) -> Self {
        self.schema_aliases.push((schema.into(), alias.into()));
        self
    }

    /// Expose common source-package-prefixed aliases used by OpenAPI Generator style SDKs.
    ///
    /// This is intentionally source-id based rather than project-specific: schemas coming from package
    /// segments such as `/dto`, `/command`, `/query`, and `/commandquery` get additional aliases like
    /// `DtoUser`, `CommandCreateUser`, `QueryLoginUser`, and `CommandqueryCreateTokenOutput`.
    #[must_use]
    pub fn legacy_source_prefixes(mut self) -> Self {
        self.legacy_source_prefixes = true;
        self
    }

    /// Whether no aliases are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.schema_aliases.is_empty()
    }

    pub(crate) fn resolve(&self, graph: &ApiGraph) -> Result<Vec<ResolvedTypeAlias>, CoreError> {
        let mut out = Vec::new();
        if self.legacy_source_prefixes {
            for schema in &graph.schemas {
                let Some(prefix) = legacy_prefix_for_schema_id(&schema.id) else {
                    continue;
                };
                if schema.name.starts_with(prefix) {
                    continue;
                }
                let alias = format!("{prefix}{}", schema.name);
                if alias == schema.name {
                    continue;
                }
                if graph
                    .schemas
                    .iter()
                    .any(|candidate| candidate.name == alias && candidate.id != schema.id)
                {
                    continue;
                }
                if out
                    .iter()
                    .any(|existing: &ResolvedTypeAlias| existing.alias == alias)
                {
                    continue;
                }
                out.push(ResolvedTypeAlias {
                    canonical: schema.name.clone(),
                    alias,
                });
            }
        }
        for (from, alias) in &self.schema_aliases {
            let matches: Vec<_> = graph
                .schemas
                .iter()
                .filter(|schema| schema.id == *from || schema.name == *from)
                .collect();
            let schema = match matches.as_slice() {
                [single] => *single,
                [] => {
                    return Err(CoreError::Config {
                        message: format!(
                            "SDK type alias source {from:?} does not match any graph schema id or name"
                        ),
                    });
                }
                many => {
                    return Err(CoreError::Config {
                        message: format!(
                            "SDK type alias source {from:?} matches {} schemas; use the full schema id",
                            many.len()
                        ),
                    });
                }
            };
            if schema.name == *alias {
                return Err(CoreError::Config {
                    message: format!(
                        "SDK type alias {alias:?} for schema '{}' duplicates the canonical name",
                        schema.name
                    ),
                });
            }
            if graph
                .schemas
                .iter()
                .any(|candidate| candidate.name == *alias && candidate.id != schema.id)
            {
                return Err(CoreError::Config {
                    message: format!(
                        "SDK type alias {alias:?} collides with an existing schema name"
                    ),
                });
            }
            if out
                .iter()
                .any(|existing: &ResolvedTypeAlias| existing.alias == *alias)
            {
                return Err(CoreError::Config {
                    message: format!("SDK type alias {alias:?} is configured more than once"),
                });
            }
            out.push(ResolvedTypeAlias {
                canonical: schema.name.clone(),
                alias: alias.clone(),
            });
        }
        Ok(out)
    }

    pub(crate) const fn uses_legacy_source_prefixes(&self) -> bool {
        self.legacy_source_prefixes
    }
}

fn legacy_prefix_for_schema_id(id: &str) -> Option<&'static str> {
    if id.contains("/dto.") || id.contains("/dto/") || id.contains("common/dto.") {
        Some("Dto")
    } else if id.contains("/commandquery.") || id.contains("/commandquery/") {
        Some("Commandquery")
    } else if id.contains("/command.") || id.contains("/command/") {
        Some("Command")
    } else if id.contains("/query.") || id.contains("/query/") {
        Some("Query")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::SdkTypeAliases;
    use crate::graph::{ApiGraph, Schema, SourceSpan, Type};

    fn graph() -> ApiGraph {
        ApiGraph {
            schemas: vec![Schema {
                id: "internal/common/dto.CreateBookInput".to_string(),
                name: "CreateBookInput".to_string(),
                body: Type::Object(vec![]),
                provenance: SourceSpan {
                    file: "models.go".to_string(),
                    start_line: 1,
                    end_line: 1,
                },
            }],
            ..ApiGraph::default()
        }
    }

    #[test]
    fn resolves_alias_from_schema_name() {
        let aliases = SdkTypeAliases::new().type_alias("CreateBookInput", "CreateBookPayload");
        let resolved = aliases.resolve(&graph()).unwrap();
        assert_eq!(resolved[0].canonical, "CreateBookInput");
        assert_eq!(resolved[0].alias, "CreateBookPayload");
    }

    #[test]
    fn resolves_legacy_source_prefix_aliases() {
        let aliases = SdkTypeAliases::new().legacy_source_prefixes();
        let resolved = aliases.resolve(&graph()).unwrap();
        assert_eq!(resolved[0].canonical, "CreateBookInput");
        assert_eq!(resolved[0].alias, "DtoCreateBookInput");
    }
}
