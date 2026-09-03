//! Deterministic structural comparison of two projected API graphs.

use std::collections::{BTreeMap, BTreeSet};

use crate::graph::direction::{
    directions_of, schema_consumers, schema_directions, SchemaConsumers, SchemaDirections,
};
use crate::graph::{
    ApiGraph, Field, OpenApiMetadataPolicy, Operation, Param, Response, Schema, SecurityScheme,
    SourceSpan, Type,
};

/// Classification of one observable API change.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Existing consumers may no longer compile or exchange the same payloads.
    Breaking,
    /// The contract accepts or provides an additional compatible surface.
    Additive,
    /// Only human-facing metadata changed.
    DocOnly,
}

/// Values for the base and current graph sides. An absent operation/schema side is `null`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Sides<T> {
    /// Value derived from the base graph when the subject exists there.
    pub base: Option<T>,
    /// Value derived from the current graph when the subject exists there.
    pub current: Option<T>,
}

/// Invocation policy recorded in the machine report.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangePolicy {
    /// Exact, case-sensitive operation tags exempted from gating.
    pub exempt_tags: Vec<String>,
}

/// Aggregate counts for a change report.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangeSummary {
    /// Number of breaking findings, whether gating or exempt.
    pub breaking: usize,
    /// Number of additive findings.
    pub additive: usize,
    /// Number of documentation-only findings.
    pub doc_only: usize,
    /// Number of breaking findings that gate this invocation.
    pub gating: usize,
}

/// One stable, auditable API change finding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Change {
    /// Compatibility classification.
    pub kind: ChangeKind,
    /// Stable dotted taxonomy code.
    pub code: String,
    /// HTTP method and absolute path when one operation can be named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Current operation id, or the base id for a removed operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    /// Parameter, field, status, schema, or other narrow subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Effective standard operation tags on each extant side.
    pub tags: Sides<Vec<String>>,
    /// Whether every known consumer is exempt on each extant side.
    pub exempt: Sides<bool>,
    /// Whether this breaking finding contributes to exit status 1.
    pub gating: bool,
    /// Human-readable explanation.
    pub message: String,
    /// Current source file, when a current fact exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Current 1-based source line, when a current fact exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Full current source span, when a current fact exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
}

/// Complete deterministic graph-diff result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChangeReport {
    /// Exact invocation policy.
    pub policy: ChangePolicy,
    /// Aggregate finding counts.
    pub summary: ChangeSummary,
    /// Findings in deterministic review order.
    pub changes: Vec<Change>,
}

impl ChangeReport {
    /// Whether at least one breaking finding gates the invocation.
    #[must_use]
    pub const fn is_gating(&self) -> bool {
        self.summary.gating > 0
    }
}

#[derive(Clone)]
struct Scope {
    operation: Option<String>,
    operation_id: Option<String>,
    tags: Sides<Vec<String>>,
    exempt: Sides<bool>,
    checked: bool,
    current_span: Option<SourceSpan>,
}

impl Scope {
    fn at(&self, current_span: Option<SourceSpan>) -> Self {
        let mut scope = self.clone();
        scope.current_span = current_span;
        scope
    }
}

struct GraphIndex<'a> {
    graph: &'a ApiGraph,
    consumers: SchemaConsumers<'a>,
    directions: BTreeMap<&'a str, SchemaDirections>,
    operation_gates: BTreeMap<&'a str, (&'a [String], bool)>,
}

impl<'a> GraphIndex<'a> {
    fn new(graph: &'a ApiGraph, exempt_tags: &'a BTreeSet<String>) -> Self {
        let resolver = crate::graph::EffectiveOperationTags::new(graph);
        let operation_gates = graph
            .operations
            .iter()
            .map(|operation| {
                let tags = resolver.resolve(operation);
                let exempt = tags.iter().any(|tag| exempt_tags.contains(tag));
                (operation.id.as_str(), (tags, exempt))
            })
            .collect();
        Self {
            graph,
            consumers: schema_consumers(graph),
            directions: schema_directions(graph),
            operation_gates,
        }
    }

    fn operation_tags(&self, operation: &Operation) -> &[String] {
        self.operation_gates
            .get(operation.id.as_str())
            .map_or(&[], |(tags, _)| *tags)
    }

    fn operation_exempt(&self, operation: &Operation) -> bool {
        self.operation_gates
            .get(operation.id.as_str())
            .is_some_and(|(_, exempt)| *exempt)
    }

    fn schema_side(&self, schema_id: &str) -> (Vec<String>, bool, Vec<&Operation>) {
        let operations: Vec<&Operation> = self
            .consumers
            .operations
            .get(schema_id)
            .into_iter()
            .flat_map(|indexes| indexes.iter())
            .filter_map(|index| self.graph.operations.get(*index))
            .collect();
        let mut tags = BTreeSet::new();
        for operation in &operations {
            tags.extend(self.operation_tags(operation).iter().cloned());
        }
        let has_non_http = self.consumers.non_http.contains(schema_id);
        let checked = has_non_http
            || operations
                .iter()
                .any(|operation| !self.operation_exempt(operation))
            || operations.is_empty();
        (tags.into_iter().collect(), !checked, operations)
    }

    fn document_side(&self) -> (Vec<String>, bool) {
        let mut tags = BTreeSet::new();
        let mut checked = false;
        for operation in &self.graph.operations {
            tags.extend(self.operation_tags(operation).iter().cloned());
            checked |= !self.operation_exempt(operation);
        }
        (tags.into_iter().collect(), !checked)
    }
}

struct Collector {
    changes: Vec<Change>,
}

impl Collector {
    fn push(
        &mut self,
        scope: &Scope,
        kind: ChangeKind,
        code: &'static str,
        subject: Option<String>,
        message: String,
    ) {
        let span = scope.current_span.clone();
        self.changes.push(Change {
            kind,
            code: code.to_string(),
            operation: scope.operation.clone(),
            operation_id: scope.operation_id.clone(),
            subject,
            tags: scope.tags.clone(),
            exempt: scope.exempt.clone(),
            gating: kind == ChangeKind::Breaking && scope.checked,
            message,
            file: span.as_ref().map(|span| span.file.clone()),
            line: span.as_ref().map(|span| span.start_line),
            span,
        });
    }
}

/// Compare two projected graphs and derive compatibility plus tag-based gating.
#[must_use]
pub fn diff_graphs(
    base: &ApiGraph,
    current: &ApiGraph,
    exempt_tags: &BTreeSet<String>,
) -> ChangeReport {
    let base_index = GraphIndex::new(base, exempt_tags);
    let current_index = GraphIndex::new(current, exempt_tags);
    let mut collector = Collector {
        changes: Vec::new(),
    };

    compare_document(&base_index, &current_index, &mut collector);
    compare_operations(&base_index, &current_index, &mut collector);
    compare_schemas(&base_index, &current_index, &mut collector);

    collector.changes.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.operation.cmp(&right.operation))
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.subject.cmp(&right.subject))
            .then_with(|| left.message.cmp(&right.message))
    });
    let mut summary = ChangeSummary::default();
    for change in &collector.changes {
        match change.kind {
            ChangeKind::Breaking => summary.breaking += 1,
            ChangeKind::Additive => summary.additive += 1,
            ChangeKind::DocOnly => summary.doc_only += 1,
        }
        if change.gating {
            summary.gating += 1;
        }
    }
    ChangeReport {
        policy: ChangePolicy {
            exempt_tags: exempt_tags.iter().cloned().collect(),
        },
        summary,
        changes: collector.changes,
    }
}

fn compare_document(base: &GraphIndex<'_>, current: &GraphIndex<'_>, out: &mut Collector) {
    let scope = document_scope(base, current);
    if base.graph.base_path != current.graph.base_path {
        out.push(
            &scope,
            ChangeKind::Breaking,
            "document.base_path.changed",
            None,
            format!(
                "base path changed from `{}` to `{}`",
                base.graph.base_path, current.graph.base_path
            ),
        );
    }
    if base.graph.title != current.graph.title {
        out.push(
            &scope,
            ChangeKind::DocOnly,
            "document.title.changed",
            None,
            "document title changed".to_string(),
        );
    }
    let mut base_metadata = base.graph.openapi_metadata.clone();
    let mut current_metadata = current.graph.openapi_metadata.clone();
    base_metadata.servers.clear();
    current_metadata.servers.clear();
    if base_metadata != current_metadata {
        out.push(
            &scope,
            ChangeKind::DocOnly,
            "document.metadata.changed",
            None,
            "document metadata changed".to_string(),
        );
    }
    compare_servers(
        &base.graph.openapi_metadata,
        &current.graph.openapi_metadata,
        &scope,
        out,
    );
    compare_security_schemes(base, current, &scope, out);
    let base_security = document_security_alternatives(base.graph);
    let current_security = document_security_alternatives(current.graph);
    if base_security != current_security {
        let kind = security_change_kind(&base_security, &current_security);
        out.push(
            &scope,
            kind,
            "security.global.changed",
            None,
            "global security requirements changed".to_string(),
        );
    }
}

fn compare_servers(
    base: &OpenApiMetadataPolicy,
    current: &OpenApiMetadataPolicy,
    scope: &Scope,
    out: &mut Collector,
) {
    let base_servers: BTreeMap<&str, Option<&str>> = base
        .servers
        .iter()
        .map(|server| (server.url.as_str(), server.description.as_deref()))
        .collect();
    let current_servers: BTreeMap<&str, Option<&str>> = current
        .servers
        .iter()
        .map(|server| (server.url.as_str(), server.description.as_deref()))
        .collect();
    for (url, description) in &base_servers {
        match current_servers.get(url) {
            None => out.push(
                &scope.at(None),
                ChangeKind::Breaking,
                "document.server.removed",
                Some((*url).to_string()),
                format!("server `{url}` removed"),
            ),
            Some(current_description) if description != current_description => out.push(
                scope,
                ChangeKind::DocOnly,
                "document.server.description.changed",
                Some((*url).to_string()),
                format!("server `{url}` description changed"),
            ),
            Some(_) => {}
        }
    }
    for url in current_servers.keys() {
        if !base_servers.contains_key(url) {
            out.push(
                scope,
                ChangeKind::Additive,
                "document.server.added",
                Some((*url).to_string()),
                format!("server `{url}` added"),
            );
        }
    }
}

fn compare_security_schemes(
    base: &GraphIndex<'_>,
    current: &GraphIndex<'_>,
    scope: &Scope,
    out: &mut Collector,
) {
    let base_schemes: BTreeMap<&str, _> = base
        .graph
        .security
        .iter()
        .map(|scheme| (scheme.id.as_str(), scheme))
        .collect();
    let current_schemes: BTreeMap<&str, _> = current
        .graph
        .security
        .iter()
        .map(|scheme| (scheme.id.as_str(), scheme))
        .collect();
    for (id, scheme) in &base_schemes {
        match current_schemes.get(id) {
            None => out.push(
                &scope.at(None),
                ChangeKind::Breaking,
                "security.scheme.removed",
                Some((*id).to_string()),
                format!("security scheme `{id}` removed"),
            ),
            Some(current_scheme)
                if security_scheme_shape(scheme) != security_scheme_shape(current_scheme) =>
            {
                out.push(
                    scope,
                    ChangeKind::Breaking,
                    "security.scheme.changed",
                    Some((*id).to_string()),
                    format!("security scheme `{id}` changed"),
                );
            }
            Some(_) => {}
        }
    }
    for id in current_schemes.keys() {
        if !base_schemes.contains_key(id) {
            out.push(
                scope,
                ChangeKind::Additive,
                "security.scheme.added",
                Some((*id).to_string()),
                format!("security scheme `{id}` added"),
            );
        }
    }
}

fn security_scheme_shape(scheme: &SecurityScheme) -> (&str, &str, &str) {
    (&scheme.kind, &scheme.location, &scheme.name)
}

fn compare_operations(base: &GraphIndex<'_>, current: &GraphIndex<'_>, out: &mut Collector) {
    let base_operations: BTreeMap<(&str, &str), &Operation> = base
        .graph
        .operations
        .iter()
        .map(|operation| {
            (
                (operation.method.as_str(), operation.path.as_str()),
                operation,
            )
        })
        .collect();
    let current_operations: BTreeMap<(&str, &str), &Operation> = current
        .graph
        .operations
        .iter()
        .map(|operation| {
            (
                (operation.method.as_str(), operation.path.as_str()),
                operation,
            )
        })
        .collect();
    let current_by_id: BTreeMap<&str, &Operation> = current
        .graph
        .operations
        .iter()
        .map(|operation| (operation.id.as_str(), operation))
        .collect();
    let mut moved_ids = BTreeSet::new();
    for (route, operation) in &base_operations {
        if let Some(current_operation) = current_operations.get(route) {
            compare_operation(base, current, operation, current_operation, out);
        } else if let Some(current_operation) = current_by_id.get(operation.id.as_str()) {
            moved_ids.insert(operation.id.as_str());
            compare_moved_operation(base, current, operation, current_operation, out);
        } else {
            let scope = operation_scope(base, current, Some(operation), None);
            out.push(
                &scope,
                ChangeKind::Breaking,
                "operation.removed",
                None,
                "operation removed".to_string(),
            );
        }
    }
    for (route, operation) in &current_operations {
        if !base_operations.contains_key(route) && !moved_ids.contains(operation.id.as_str()) {
            let scope = operation_scope(base, current, None, Some(operation));
            out.push(
                &scope,
                ChangeKind::Additive,
                "operation.added",
                None,
                "operation added".to_string(),
            );
        }
    }
}

fn compare_moved_operation(
    base_index: &GraphIndex<'_>,
    current_index: &GraphIndex<'_>,
    base: &Operation,
    current: &Operation,
    out: &mut Collector,
) {
    let scope = operation_scope(base_index, current_index, Some(base), Some(current));
    if base.path != current.path {
        out.push(
            &scope,
            ChangeKind::Breaking,
            "operation.path.changed",
            None,
            format!(
                "operation path changed from `{}` to `{}`",
                join_path(&base_index.graph.base_path, &base.path),
                join_path(&current_index.graph.base_path, &current.path)
            ),
        );
    }
    if base.method != current.method {
        out.push(
            &scope,
            ChangeKind::Breaking,
            "operation.method.changed",
            None,
            format!(
                "operation method changed from `{}` to `{}`",
                base.method, current.method
            ),
        );
    }
    compare_operation(base_index, current_index, base, current, out);
}

fn compare_operation(
    base_index: &GraphIndex<'_>,
    current_index: &GraphIndex<'_>,
    base: &Operation,
    current: &Operation,
    out: &mut Collector,
) {
    let scope = operation_scope(base_index, current_index, Some(base), Some(current));
    if effective_operation_name(base_index.graph, base)
        != effective_operation_name(current_index.graph, current)
    {
        out.push(
            &scope,
            ChangeKind::Breaking,
            "operation.name.changed",
            None,
            format!(
                "operation name changed from `{}` to `{}`",
                effective_operation_name(base_index.graph, base),
                effective_operation_name(current_index.graph, current)
            ),
        );
    }
    if base.group != current.group {
        out.push(
            &scope,
            ChangeKind::Breaking,
            "sdk.group.changed",
            None,
            format!(
                "SDK group changed from `{}` to `{}`",
                base.group.as_deref().unwrap_or("default"),
                current.group.as_deref().unwrap_or("default")
            ),
        );
    }
    compare_operation_tags(base_index, current_index, base, current, &scope, out);
    if operation_documentation(base_index.graph, base)
        != operation_documentation(current_index.graph, current)
    {
        out.push(
            &scope,
            ChangeKind::DocOnly,
            "operation.documentation.changed",
            None,
            "operation documentation changed".to_string(),
        );
    }
    compare_parameters(base, current, &scope, out);
    compare_request_body(
        base_index.graph,
        current_index.graph,
        base,
        current,
        &scope,
        out,
    );
    compare_responses(base, current, &scope, out);
    compare_operation_security(base_index, current_index, base, current, &scope, out);
}

fn compare_operation_tags(
    base_index: &GraphIndex<'_>,
    current_index: &GraphIndex<'_>,
    base: &Operation,
    current: &Operation,
    scope: &Scope,
    out: &mut Collector,
) {
    let base_tags = base_index.operation_tags(base);
    let current_tags = current_index.operation_tags(current);
    if base_tags == current_tags {
        return;
    }
    let base_exempt = base_index.operation_exempt(base);
    let current_exempt = current_index.operation_exempt(current);
    let (kind, code, message) = match (base_exempt, current_exempt) {
        (false, true) => (
            ChangeKind::Breaking,
            "operation.exemption.added",
            "operation moved from checked to exempt scope".to_string(),
        ),
        (true, false) => (
            ChangeKind::Additive,
            "operation.exemption.removed",
            "operation moved from exempt to checked scope".to_string(),
        ),
        _ => (
            ChangeKind::DocOnly,
            "operation.tags.changed",
            "operation tags changed".to_string(),
        ),
    };
    out.push(scope, kind, code, None, message);
}

fn compare_parameters(base: &Operation, current: &Operation, scope: &Scope, out: &mut Collector) {
    let base_params: BTreeMap<(&str, &str), &Param> = base
        .params
        .iter()
        .map(|param| ((param.location.as_str(), param.name.as_str()), param))
        .collect();
    let current_params: BTreeMap<(&str, &str), &Param> = current
        .params
        .iter()
        .map(|param| ((param.location.as_str(), param.name.as_str()), param))
        .collect();
    for (key, parameter) in &base_params {
        let subject = format!("{} {}", parameter.location, parameter.name);
        match current_params.get(key) {
            None => out.push(
                &scope.at(None),
                ChangeKind::Breaking,
                "request.parameter.removed",
                Some(subject),
                format!(
                    "{} parameter `{}` removed",
                    parameter.location, parameter.name
                ),
            ),
            Some(current_parameter) => {
                compare_existing_parameter(
                    parameter,
                    current_parameter,
                    &subject,
                    &scope.at(Some(current_parameter.provenance.clone())),
                    out,
                );
            }
        }
    }
    for (key, parameter) in &current_params {
        if !base_params.contains_key(key) {
            let kind = if parameter.required {
                ChangeKind::Breaking
            } else {
                ChangeKind::Additive
            };
            let code = if parameter.required {
                "request.parameter.required.added"
            } else {
                "request.parameter.added"
            };
            out.push(
                &scope.at(Some(parameter.provenance.clone())),
                kind,
                code,
                Some(format!("{} {}", parameter.location, parameter.name)),
                format!(
                    "{} {} parameter `{}` added",
                    if parameter.required {
                        "required"
                    } else {
                        "optional"
                    },
                    parameter.location,
                    parameter.name
                ),
            );
        }
    }
}

fn compare_existing_parameter(
    base: &Param,
    current: &Param,
    subject: &str,
    scope: &Scope,
    out: &mut Collector,
) {
    if base.required != current.required {
        let (kind, code, message) = if current.required {
            (
                ChangeKind::Breaking,
                "request.parameter.required.added",
                format!("parameter `{}` became required", base.name),
            )
        } else {
            (
                ChangeKind::Additive,
                "request.parameter.required.removed",
                format!("parameter `{}` became optional", base.name),
            )
        };
        out.push(scope, kind, code, Some(subject.to_string()), message);
    }
    compare_type(
        &base.schema,
        &current.schema,
        subject,
        TypeDirections::request(),
        scope,
        out,
    );
    if base.default != current.default {
        out.push(
            scope,
            ChangeKind::Breaking,
            "request.parameter.default.changed",
            Some(subject.to_string()),
            format!("parameter `{}` default changed", base.name),
        );
    }
    let base_openapi = parameter_openapi_value(base);
    let current_openapi = parameter_openapi_value(current);
    let mut base_structural = base_openapi.clone();
    let mut current_structural = current_openapi.clone();
    strip_documentation(&mut base_structural);
    strip_documentation(&mut current_structural);
    if base.style != current.style
        || base.explode != current.explode
        || base.allow_reserved != current.allow_reserved
        || base_structural != current_structural
    {
        out.push(
            scope,
            ChangeKind::Breaking,
            "request.parameter.serialization.changed",
            Some(subject.to_string()),
            format!("parameter `{}` serialization changed", base.name),
        );
    } else if base_openapi != current_openapi {
        out.push(
            scope,
            ChangeKind::DocOnly,
            "request.parameter.documentation.changed",
            Some(subject.to_string()),
            format!("parameter `{}` documentation changed", base.name),
        );
    }
}

fn parameter_openapi_value(parameter: &Param) -> serde_json::Value {
    let fields: serde_json::Map<String, serde_json::Value> =
        parameter.openapi_fields.iter().cloned().collect();
    serde_json::json!({ "content": parameter.openapi_content, "fields": fields })
}

fn strip_documentation(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(fields) => {
            for key in ["deprecated", "description", "example", "examples", "title"] {
                fields.remove(key);
            }
            for child in fields.values_mut() {
                strip_documentation(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                strip_documentation(child);
            }
        }
        _ => {}
    }
}

fn compare_request_body(
    base_graph: &ApiGraph,
    current_graph: &ApiGraph,
    base: &Operation,
    current: &Operation,
    scope: &Scope,
    out: &mut Collector,
) {
    match (&base.request_body, &current.request_body) {
        (Some(_), None) => out.push(
            &scope.at(None),
            ChangeKind::Breaking,
            "request.body.removed",
            None,
            "request body removed".to_string(),
        ),
        (None, Some(_)) => out.push(
            scope,
            if current.request_body_required {
                ChangeKind::Breaking
            } else {
                ChangeKind::Additive
            },
            if current.request_body_required {
                "request.body.required.added"
            } else {
                "request.body.added"
            },
            None,
            format!(
                "{} request body added",
                if current.request_body_required {
                    "required"
                } else {
                    "optional"
                }
            ),
        ),
        (Some(base_body), Some(current_body)) => {
            if base_body.ref_id != current_body.ref_id {
                out.push(
                    scope,
                    ChangeKind::Breaking,
                    "request.body.schema.changed",
                    None,
                    format!(
                        "request body schema changed from `{}` to `{}`",
                        base_body.ref_id, current_body.ref_id
                    ),
                );
            }
            if base.request_body_required != current.request_body_required {
                let (kind, code, message) = if current.request_body_required {
                    (
                        ChangeKind::Breaking,
                        "request.body.required.added",
                        "request body became required",
                    )
                } else {
                    (
                        ChangeKind::Additive,
                        "request.body.required.removed",
                        "request body became optional",
                    )
                };
                out.push(scope, kind, code, None, message.to_string());
            }
        }
        (None, None) => {}
    }
    compare_string_sets(
        &request_content_types(base_graph, base),
        &request_content_types(current_graph, current),
        "request.body.media_type.removed",
        "request.body.media_type.added",
        "request body media type",
        scope,
        out,
        ChangeKind::Breaking,
        ChangeKind::Additive,
    );
}

fn compare_responses(base: &Operation, current: &Operation, scope: &Scope, out: &mut Collector) {
    let base_responses: BTreeMap<u16, &Response> = base
        .responses
        .iter()
        .map(|response| (response.status, response))
        .collect();
    let current_responses: BTreeMap<u16, &Response> = current
        .responses
        .iter()
        .map(|response| (response.status, response))
        .collect();
    for (status, response) in &base_responses {
        let subject = status.to_string();
        match current_responses.get(status) {
            None => out.push(
                &scope.at(None),
                ChangeKind::Breaking,
                "response.status.removed",
                Some(subject),
                format!("response status {status} removed"),
            ),
            Some(current_response) => {
                match (&response.body, &current_response.body) {
                    (Some(_), None) => out.push(
                        &scope.at(None),
                        ChangeKind::Breaking,
                        "response.body.removed",
                        Some(subject.clone()),
                        format!("response body for status {status} removed"),
                    ),
                    (None, Some(_)) => out.push(
                        scope,
                        ChangeKind::Breaking,
                        "response.body.added",
                        Some(subject.clone()),
                        format!("response body for status {status} added"),
                    ),
                    (Some(base_body), Some(current_body))
                        if base_body.ref_id != current_body.ref_id =>
                    {
                        out.push(
                            scope,
                            ChangeKind::Breaking,
                            "response.body.schema.changed",
                            Some(subject.clone()),
                            format!("response schema for status {status} changed"),
                        );
                    }
                    _ => {}
                }
                if response.body_kind != current_response.body_kind {
                    out.push(
                        scope,
                        ChangeKind::Breaking,
                        "response.body.kind.changed",
                        Some(subject.clone()),
                        format!("response body kind for status {status} changed"),
                    );
                }
                compare_string_sets(
                    &response_content_types(response),
                    &response_content_types(current_response),
                    "response.media_type.removed",
                    "response.media_type.added",
                    &format!("response {status} media type"),
                    scope,
                    out,
                    ChangeKind::Breaking,
                    ChangeKind::Additive,
                );
            }
        }
    }
    for status in current_responses.keys() {
        if !base_responses.contains_key(status) {
            out.push(
                scope,
                ChangeKind::Additive,
                "response.status.added",
                Some(status.to_string()),
                format!("response status {status} added"),
            );
        }
    }
}

fn compare_operation_security(
    base_index: &GraphIndex<'_>,
    current_index: &GraphIndex<'_>,
    base: &Operation,
    current: &Operation,
    scope: &Scope,
    out: &mut Collector,
) {
    let base_security = normalized_security_groups(
        crate::sdk::emit_common::operation_security_alternatives(base_index.graph, base),
    );
    let current_security = normalized_security_groups(
        crate::sdk::emit_common::operation_security_alternatives(current_index.graph, current),
    );
    if base_security == current_security {
        return;
    }
    let kind = security_change_kind(&base_security, &current_security);
    let (code, message) = if security_is_public(&base_security) {
        (
            "security.operation.added",
            "operation now requires security",
        )
    } else if security_is_public(&current_security) {
        (
            "security.operation.removed",
            "operation no longer requires security",
        )
    } else {
        (
            "security.operation.changed",
            "operation security requirements changed",
        )
    };
    out.push(scope, kind, code, None, message.to_string());
}

fn document_security_alternatives(graph: &ApiGraph) -> Vec<Vec<String>> {
    let groups = if graph.security_requirements.is_empty() {
        let schemes: Vec<String> = graph
            .security
            .iter()
            .filter(|scheme| scheme.global)
            .map(|scheme| scheme.id.clone())
            .collect();
        if schemes.is_empty() {
            Vec::new()
        } else {
            vec![schemes]
        }
    } else {
        graph
            .security_requirements
            .iter()
            .map(|group| group.schemes.clone())
            .collect()
    };
    normalized_security_groups(groups)
}

fn normalized_security_groups(mut groups: Vec<Vec<String>>) -> Vec<Vec<String>> {
    for group in &mut groups {
        group.sort();
        group.dedup();
    }
    groups.sort();
    groups.dedup();
    if groups.is_empty() || groups.iter().any(Vec::is_empty) {
        vec![Vec::new()]
    } else {
        groups
    }
}

fn security_change_kind(base: &[Vec<String>], current: &[Vec<String>]) -> ChangeKind {
    if security_is_public(base) {
        return ChangeKind::Breaking;
    }
    if security_is_public(current) {
        return ChangeKind::Additive;
    }
    let current_is_at_least_as_permissive = base.iter().all(|base_group| {
        current.iter().any(|current_group| {
            current_group
                .iter()
                .all(|scheme| base_group.binary_search(scheme).is_ok())
        })
    });
    if current_is_at_least_as_permissive {
        ChangeKind::Additive
    } else {
        ChangeKind::Breaking
    }
}

fn security_is_public(groups: &[Vec<String>]) -> bool {
    groups.iter().any(Vec::is_empty)
}

fn compare_schemas(base: &GraphIndex<'_>, current: &GraphIndex<'_>, out: &mut Collector) {
    let base_schemas: BTreeMap<&str, &Schema> = base
        .graph
        .schemas
        .iter()
        .map(|schema| (schema.id.as_str(), schema))
        .collect();
    let current_schemas: BTreeMap<&str, &Schema> = current
        .graph
        .schemas
        .iter()
        .map(|schema| (schema.id.as_str(), schema))
        .collect();
    for (id, schema) in &base_schemas {
        match current_schemas.get(id) {
            None => {
                let scope = schema_scope(base, current, Some(schema), None);
                out.push(
                    &scope,
                    ChangeKind::Breaking,
                    "schema.removed",
                    Some((*id).to_string()),
                    format!("schema `{}` removed", schema.name),
                );
            }
            Some(current_schema) => {
                let scope = schema_scope(base, current, Some(schema), Some(current_schema));
                if schema.name != current_schema.name {
                    out.push(
                        &scope,
                        ChangeKind::Breaking,
                        "schema.name.changed",
                        Some((*id).to_string()),
                        format!(
                            "schema name changed from `{}` to `{}`",
                            schema.name, current_schema.name
                        ),
                    );
                }
                compare_type(
                    &schema.body,
                    &current_schema.body,
                    &schema.name,
                    TypeDirections {
                        base: directions_of(&base.directions, id),
                        current: directions_of(&current.directions, id),
                    },
                    &scope,
                    out,
                );
                if schema.enum_source_order != current_schema.enum_source_order
                    || enum_order_changed(&schema.body, &current_schema.body)
                {
                    out.push(
                        &scope,
                        ChangeKind::DocOnly,
                        "schema.enum.order.changed",
                        Some((*id).to_string()),
                        format!("schema `{}` enum declaration order changed", schema.name),
                    );
                }
            }
        }
    }
    for (id, schema) in &current_schemas {
        if !base_schemas.contains_key(id) {
            let scope = schema_scope(base, current, None, Some(schema));
            out.push(
                &scope,
                ChangeKind::Additive,
                "schema.added",
                Some((*id).to_string()),
                format!("schema `{}` added", schema.name),
            );
        }
    }
}

fn enum_order_changed(base: &Type, current: &Type) -> bool {
    match (base, current) {
        (Type::Enum(base), Type::Enum(current)) => {
            base != current
                && base.iter().collect::<BTreeSet<_>>() == current.iter().collect::<BTreeSet<_>>()
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct TypeDirections {
    base: SchemaDirections,
    current: SchemaDirections,
}

impl TypeDirections {
    const fn request() -> Self {
        Self {
            base: SchemaDirections::REQUEST,
            current: SchemaDirections::REQUEST,
        }
    }

    fn request_or_unconsumed(self) -> bool {
        self.base.request || self.current.request || (!self.base.response && !self.current.response)
    }

    fn response(self) -> bool {
        self.base.response || self.current.response
    }

    fn unconsumed(self) -> bool {
        !self.base.request && !self.current.request && !self.base.response && !self.current.response
    }

    fn prefix(self, prefer_request: bool) -> &'static str {
        if prefer_request && (self.base.request || self.current.request) {
            "request"
        } else if self.response() {
            "response"
        } else {
            "schema"
        }
    }
}

fn compare_type(
    base: &Type,
    current: &Type,
    subject: &str,
    directions: TypeDirections,
    scope: &Scope,
    out: &mut Collector,
) {
    match (base, current) {
        (Type::Object(base_fields), Type::Object(current_fields)) => {
            compare_fields(base_fields, current_fields, subject, directions, scope, out);
        }
        (Type::Enum(base_values), Type::Enum(current_values)) => {
            compare_enum(base_values, current_values, subject, directions, scope, out);
        }
        (Type::Array(base_item), Type::Array(current_item)) => compare_type(
            base_item,
            current_item,
            &format!("{subject}[]"),
            directions,
            scope,
            out,
        ),
        (
            Type::Map {
                key: base_key,
                value: base_value,
            },
            Type::Map {
                key: current_key,
                value: current_value,
            },
        ) => {
            compare_type(
                base_key,
                current_key,
                &format!("{subject}.key"),
                directions,
                scope,
                out,
            );
            compare_type(
                base_value,
                current_value,
                &format!("{subject}.value"),
                directions,
                scope,
                out,
            );
        }
        _ if base == current => {}
        _ => {
            let prefix = directions.prefix(true);
            out.push(
                scope,
                ChangeKind::Breaking,
                type_code(prefix, "type.changed"),
                Some(subject.to_string()),
                format!("{prefix} type `{subject}` changed"),
            );
        }
    }
}

fn compare_fields(
    base_fields: &[Field],
    current_fields: &[Field],
    parent: &str,
    directions: TypeDirections,
    scope: &Scope,
    out: &mut Collector,
) {
    let base_map: BTreeMap<&str, &Field> = base_fields
        .iter()
        .map(|field| (field.json_name.as_str(), field))
        .collect();
    let current_map: BTreeMap<&str, &Field> = current_fields
        .iter()
        .map(|field| (field.json_name.as_str(), field))
        .collect();
    for (name, field) in &base_map {
        let subject = format!("{parent}.{name}");
        match current_map.get(name) {
            None => {
                let prefix = directions.prefix(true);
                out.push(
                    &scope.at(None),
                    ChangeKind::Breaking,
                    property_code(prefix, "removed"),
                    Some(subject),
                    format!("{prefix} field `{name}` removed"),
                );
            }
            Some(current_field) => {
                compare_field_axes(field, current_field, name, &subject, directions, scope, out);
                compare_type(
                    &field.schema,
                    &current_field.schema,
                    &subject,
                    directions,
                    scope,
                    out,
                );
                if field.description != current_field.description
                    || field.example != current_field.example
                {
                    out.push(
                        scope,
                        ChangeKind::DocOnly,
                        "schema.property.documentation.changed",
                        Some(subject.clone()),
                        format!("field `{name}` documentation changed"),
                    );
                }
                if field.meta != current_field.meta {
                    let prefix = directions.prefix(true);
                    out.push(
                        scope,
                        ChangeKind::Breaking,
                        property_code(prefix, "constraints.changed"),
                        Some(subject),
                        format!("{prefix} field `{name}` constraints changed"),
                    );
                }
            }
        }
    }
    for (name, field) in &current_map {
        if !base_map.contains_key(name) {
            let required = required_on(field, directions.current);
            let request_break = required && directions.request_or_unconsumed();
            let prefix = directions.prefix(request_break);
            out.push(
                scope,
                if request_break {
                    ChangeKind::Breaking
                } else {
                    ChangeKind::Additive
                },
                property_code(prefix, "added"),
                Some(format!("{parent}.{name}")),
                format!(
                    "{} {prefix} field `{name}` added",
                    if required { "required" } else { "optional" }
                ),
            );
        }
    }
}

fn compare_field_axes(
    base: &Field,
    current: &Field,
    name: &str,
    subject: &str,
    directions: TypeDirections,
    scope: &Scope,
    out: &mut Collector,
) {
    let base_required = required_on(base, directions.base);
    let current_required = required_on(current, directions.current);
    if base_required != current_required {
        let added = current_required;
        let breaking = directions.unconsumed()
            || if added {
                directions.request_or_unconsumed()
            } else {
                directions.response()
            };
        let prefix = directions.prefix(added);
        out.push(
            scope,
            if breaking {
                ChangeKind::Breaking
            } else {
                ChangeKind::Additive
            },
            property_code(
                prefix,
                if added {
                    "required.added"
                } else {
                    "required.removed"
                },
            ),
            Some(subject.to_string()),
            format!(
                "{prefix} field `{name}` changed from {} to {}",
                if base_required {
                    "required"
                } else {
                    "optional"
                },
                if current_required {
                    "required"
                } else {
                    "optional"
                }
            ),
        );
    }
    let base_nullable = nullable_on(base, directions.base);
    let current_nullable = nullable_on(current, directions.current);
    if base_nullable != current_nullable {
        let added = current_nullable;
        let breaking = directions.unconsumed()
            || if added {
                directions.response()
            } else {
                directions.request_or_unconsumed()
            };
        let prefix = directions.prefix(!added);
        out.push(
            scope,
            if breaking {
                ChangeKind::Breaking
            } else {
                ChangeKind::Additive
            },
            property_code(
                prefix,
                if added {
                    "nullability.added"
                } else {
                    "nullability.removed"
                },
            ),
            Some(subject.to_string()),
            format!(
                "{prefix} field `{name}` {} null",
                if added {
                    "now accepts"
                } else {
                    "no longer accepts"
                }
            ),
        );
    }
}

fn compare_enum(
    base: &[String],
    current: &[String],
    subject: &str,
    directions: TypeDirections,
    scope: &Scope,
    out: &mut Collector,
) {
    let base: BTreeSet<&str> = base.iter().map(String::as_str).collect();
    let current: BTreeSet<&str> = current.iter().map(String::as_str).collect();
    for value in base.difference(&current) {
        let breaking = directions.request_or_unconsumed();
        let prefix = directions.prefix(true);
        out.push(
            &scope.at(None),
            if breaking {
                ChangeKind::Breaking
            } else {
                ChangeKind::Additive
            },
            type_code(prefix, "enum.value.removed"),
            Some(subject.to_string()),
            format!("{prefix} enum value `{value}` removed from `{subject}`"),
        );
    }
    for value in current.difference(&base) {
        let breaking = directions.response() || directions.unconsumed();
        let prefix = directions.prefix(false);
        out.push(
            scope,
            if breaking {
                ChangeKind::Breaking
            } else {
                ChangeKind::Additive
            },
            type_code(prefix, "enum.value.added"),
            Some(subject.to_string()),
            format!("{prefix} enum value `{value}` added to `{subject}`"),
        );
    }
}

fn required_on(field: &Field, directions: SchemaDirections) -> bool {
    directions.field_is_required(field)
}

fn nullable_on(field: &Field, directions: SchemaDirections) -> bool {
    directions.field_is_nullable(field)
}

fn property_code(prefix: &str, suffix: &str) -> &'static str {
    match (prefix, suffix) {
        ("request", "added") => "request.property.added",
        ("request", "removed") => "request.property.removed",
        ("request", "required.added") => "request.property.required.added",
        ("request", "required.removed") => "request.property.required.removed",
        ("request", "nullability.added") => "request.property.nullability.added",
        ("request", "nullability.removed") => "request.property.nullability.removed",
        ("request", "constraints.changed") => "request.property.constraints.changed",
        ("response", "added") => "response.property.added",
        ("response", "removed") => "response.property.removed",
        ("response", "required.added") => "response.property.required.added",
        ("response", "required.removed") => "response.property.required.removed",
        ("response", "nullability.added") => "response.property.nullability.added",
        ("response", "nullability.removed") => "response.property.nullability.removed",
        ("response", "constraints.changed") => "response.property.constraints.changed",
        (_, "added") => "schema.property.added",
        (_, "removed") => "schema.property.removed",
        (_, "required.added") => "schema.property.required.added",
        (_, "required.removed") => "schema.property.required.removed",
        (_, "nullability.added") => "schema.property.nullability.added",
        (_, "nullability.removed") => "schema.property.nullability.removed",
        _ => "schema.property.constraints.changed",
    }
}

fn type_code(prefix: &str, suffix: &str) -> &'static str {
    match (prefix, suffix) {
        ("request", "type.changed") => "request.type.changed",
        ("response", "type.changed") => "response.type.changed",
        ("request", "enum.value.added") => "request.enum.value.added",
        ("request", "enum.value.removed") => "request.enum.value.removed",
        ("response", "enum.value.added") => "response.enum.value.added",
        ("response", "enum.value.removed") => "response.enum.value.removed",
        (_, "enum.value.added") => "schema.enum.value.added",
        (_, "enum.value.removed") => "schema.enum.value.removed",
        _ => "schema.type.changed",
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_string_sets(
    base: &BTreeSet<String>,
    current: &BTreeSet<String>,
    removed_code: &'static str,
    added_code: &'static str,
    label: &str,
    scope: &Scope,
    out: &mut Collector,
    removed_kind: ChangeKind,
    added_kind: ChangeKind,
) {
    for value in base.difference(current) {
        out.push(
            &scope.at(None),
            removed_kind,
            removed_code,
            Some(value.clone()),
            format!("{label} `{value}` removed"),
        );
    }
    for value in current.difference(base) {
        out.push(
            scope,
            added_kind,
            added_code,
            Some(value.clone()),
            format!("{label} `{value}` added"),
        );
    }
}

fn request_content_types(graph: &ApiGraph, operation: &Operation) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    if let Some(content_type) = &operation.request_body_content_type {
        values.insert(content_type.clone());
    }
    if let Some(policy) = operation_docs_policy(graph, &operation.id) {
        values.extend(policy.request_content_types.iter().cloned());
    }
    values
}

fn response_content_types(response: &Response) -> BTreeSet<String> {
    response
        .content_type
        .iter()
        .chain(response.content_types.iter())
        .cloned()
        .collect()
}

fn effective_operation_name<'a>(graph: &'a ApiGraph, operation: &'a Operation) -> &'a str {
    operation_docs_policy(graph, &operation.id)
        .and_then(|policy| policy.openapi_operation_id.as_deref())
        .unwrap_or(&operation.id)
}

fn operation_documentation<'a>(
    graph: &'a ApiGraph,
    operation: &'a Operation,
) -> OperationDocumentation<'a> {
    let policy = operation_docs_policy(graph, &operation.id);
    OperationDocumentation {
        summary: operation.summary.as_deref(),
        description: operation.description.as_deref(),
        deprecated: policy.is_some_and(|policy| policy.deprecated),
        request_examples: policy.map(|policy| policy.request_examples.as_slice()),
        responses: policy.map(|policy| policy.responses.as_slice()),
    }
}

#[derive(PartialEq)]
struct OperationDocumentation<'a> {
    summary: Option<&'a str>,
    description: Option<&'a str>,
    deprecated: bool,
    request_examples: Option<&'a [crate::graph::MediaExample]>,
    responses: Option<&'a [crate::graph::ResponseDocsPolicy]>,
}

fn operation_docs_policy<'a>(
    graph: &'a ApiGraph,
    operation_id: &str,
) -> Option<&'a crate::graph::OperationDocsPolicy> {
    graph
        .operation_docs
        .iter()
        .find(|policy| policy.operation_id == operation_id)
}

fn operation_scope(
    base_index: &GraphIndex<'_>,
    current_index: &GraphIndex<'_>,
    base: Option<&Operation>,
    current: Option<&Operation>,
) -> Scope {
    let base_tags = base.map(|operation| base_index.operation_tags(operation).to_vec());
    let current_tags = current.map(|operation| current_index.operation_tags(operation).to_vec());
    let base_exempt = base.map(|operation| base_index.operation_exempt(operation));
    let current_exempt = current.map(|operation| current_index.operation_exempt(operation));
    Scope {
        operation: current
            .map(|operation| operation_label(current_index.graph, operation))
            .or_else(|| base.map(|operation| operation_label(base_index.graph, operation))),
        operation_id: current
            .map(|operation| operation.id.clone())
            .or_else(|| base.map(|operation| operation.id.clone())),
        tags: Sides {
            base: base_tags,
            current: current_tags,
        },
        exempt: Sides {
            base: base_exempt,
            current: current_exempt,
        },
        checked: base_exempt.is_some_and(|value| !value)
            || current_exempt.is_some_and(|value| !value),
        current_span: current.map(|operation| operation.provenance.clone()),
    }
}

fn schema_scope(
    base_index: &GraphIndex<'_>,
    current_index: &GraphIndex<'_>,
    base: Option<&Schema>,
    current: Option<&Schema>,
) -> Scope {
    let base_side = base.map(|schema| base_index.schema_side(&schema.id));
    let current_side = current.map(|schema| current_index.schema_side(&schema.id));
    let base_exempt = base_side.as_ref().map(|(_, exempt, _)| *exempt);
    let current_exempt = current_side.as_ref().map(|(_, exempt, _)| *exempt);
    let current_operation = current_side
        .as_ref()
        .and_then(|(_, _, operations)| single_operation(current_index.graph, operations));
    let base_operation = base_side
        .as_ref()
        .and_then(|(_, _, operations)| single_operation(base_index.graph, operations));
    Scope {
        operation: current_operation
            .as_ref()
            .map(|(label, _)| label.clone())
            .or_else(|| base_operation.as_ref().map(|(label, _)| label.clone())),
        operation_id: current_operation
            .map(|(_, id)| id)
            .or_else(|| base_operation.map(|(_, id)| id)),
        tags: Sides {
            base: base_side.as_ref().map(|(tags, _, _)| tags.clone()),
            current: current_side.as_ref().map(|(tags, _, _)| tags.clone()),
        },
        exempt: Sides {
            base: base_exempt,
            current: current_exempt,
        },
        checked: base_exempt.is_some_and(|value| !value)
            || current_exempt.is_some_and(|value| !value),
        current_span: current.map(|schema| schema.provenance.clone()),
    }
}

fn single_operation(graph: &ApiGraph, operations: &[&Operation]) -> Option<(String, String)> {
    let [operation] = operations else {
        return None;
    };
    Some((operation_label(graph, operation), operation.id.clone()))
}

fn document_scope(base: &GraphIndex<'_>, current: &GraphIndex<'_>) -> Scope {
    let (base_tags, base_exempt) = base.document_side();
    let (current_tags, current_exempt) = current.document_side();
    Scope {
        operation: None,
        operation_id: None,
        tags: Sides {
            base: Some(base_tags),
            current: Some(current_tags),
        },
        exempt: Sides {
            base: Some(base_exempt),
            current: Some(current_exempt),
        },
        checked: !base_exempt || !current_exempt,
        current_span: None,
    }
}

fn operation_label(graph: &ApiGraph, operation: &Operation) -> String {
    format!(
        "{} {}",
        operation.method,
        join_path(&graph.base_path, &operation.path)
    )
}

fn join_path(base: &str, path: &str) -> String {
    if base == "/" {
        return format!("/{}", path.trim_start_matches('/'));
    }
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::too_many_lines)]

    use std::collections::BTreeSet;

    use super::{diff_graphs, ChangeKind};
    use crate::analyze::facts::FieldMeta;
    use crate::graph::{
        ApiGraph, Field, Operation, OperationDocsPolicy, Param, Prim, Response, Schema, SchemaRef,
        SchemaUse, SchemaUseRoot, SecurityScheme, SourceSpan, Type,
    };

    fn span(file: &str) -> SourceSpan {
        SourceSpan {
            file: file.to_string(),
            start_line: 10,
            end_line: 12,
        }
    }

    fn operation() -> Operation {
        Operation {
            id: "listBooks".to_string(),
            method: "GET".to_string(),
            path: "/books".to_string(),
            handler: "listBooks".to_string(),
            summary: None,
            description: None,
            group: None,
            middleware: Vec::new(),
            params: Vec::new(),
            request_body: None,
            request_body_required: true,
            request_body_content_type: None,
            responses: Vec::new(),
            security: Vec::new(),
            security_overrides_global: false,
            provenance: span("handlers.rs"),
        }
    }

    fn policy(operation_id: &str, tags: &[&str]) -> OperationDocsPolicy {
        OperationDocsPolicy {
            operation_id: operation_id.to_string(),
            openapi_operation_id: None,
            deprecated: false,
            tags: tags.iter().map(ToString::to_string).collect(),
            request_examples: Vec::new(),
            request_content_types: Vec::new(),
            responses: Vec::new(),
        }
    }

    fn graph_with_tags(tags: &[&str]) -> ApiGraph {
        let operation = operation();
        ApiGraph {
            operations: vec![operation],
            operation_docs: (!tags.is_empty())
                .then(|| policy("listBooks", tags))
                .into_iter()
                .collect(),
            ..ApiGraph::default()
        }
    }

    fn exemptions(tags: &[&str]) -> BTreeSet<String> {
        tags.iter().map(ToString::to_string).collect()
    }

    fn change<'a>(report: &'a super::ChangeReport, code: &str) -> &'a super::Change {
        report
            .changes
            .iter()
            .find(|change| change.code == code)
            .unwrap_or_else(|| panic!("missing {code}: {:?}", report.changes))
    }

    #[test]
    fn gate_transition_table_is_exhaustive() {
        let checked = graph_with_tags(&["books"]);
        let checked_other = graph_with_tags(&["reports"]);
        let exempt = graph_with_tags(&["internal"]);
        let exempt_other = graph_with_tags(&["beta"]);
        let empty = ApiGraph::default();
        let policy = exemptions(&["internal", "beta"]);

        assert!(diff_graphs(&empty, &empty, &policy).changes.is_empty());

        for added in [&checked, &exempt] {
            let finding = change(&diff_graphs(&empty, added, &policy), "operation.added").clone();
            assert_eq!(finding.kind, ChangeKind::Additive);
            assert!(!finding.gating);
        }

        let checked_removed = diff_graphs(&checked, &empty, &policy);
        assert!(change(&checked_removed, "operation.removed").gating);
        let exempt_removed = diff_graphs(&exempt, &empty, &policy);
        assert!(!change(&exempt_removed, "operation.removed").gating);

        let checked_to_checked = diff_graphs(&checked, &checked_other, &policy);
        assert_eq!(
            change(&checked_to_checked, "operation.tags.changed").kind,
            ChangeKind::DocOnly
        );
        let checked_to_exempt = diff_graphs(&checked, &exempt, &policy);
        let narrowed = change(&checked_to_exempt, "operation.exemption.added");
        assert_eq!(narrowed.kind, ChangeKind::Breaking);
        assert!(narrowed.gating);
        let exempt_to_checked = diff_graphs(&exempt, &checked, &policy);
        assert_eq!(
            change(&exempt_to_checked, "operation.exemption.removed").kind,
            ChangeKind::Additive
        );
        let exempt_to_exempt = diff_graphs(&exempt, &exempt_other, &policy);
        assert_eq!(
            change(&exempt_to_exempt, "operation.tags.changed").kind,
            ChangeKind::DocOnly
        );
    }

    fn structural_break(tags: &[&str], current_tags: &[&str], exempt: &[&str]) -> bool {
        let mut base = graph_with_tags(tags);
        let mut current = graph_with_tags(current_tags);
        base.operations[0].params.push(parameter(false));
        current.operations[0].params.push(parameter(true));
        let report = diff_graphs(&base, &current, &exemptions(exempt));
        change(&report, "request.parameter.required.added").gating
    }

    #[test]
    fn multiple_exempt_tags_use_any_match_per_side_and_both_sides_overall() {
        let exempt = ["internal", "beta", "partner"];
        let cases = [
            ((vec!["books"], vec!["books"]), true),
            ((vec!["books"], vec!["books", "internal"]), true),
            ((vec!["books", "beta"], vec!["books", "internal"]), false),
            ((vec!["partner", "beta"], vec!["partner"]), false),
            ((vec!["partner"], vec!["books"]), true),
            ((Vec::new(), vec!["internal"]), true),
        ];
        for ((base, current), expected) in cases {
            assert_eq!(
                structural_break(&base, &current, &exempt),
                expected,
                "base={base:?} current={current:?}"
            );
        }
    }

    fn parameter(required: bool) -> Param {
        Param {
            name: "limit".to_string(),
            location: "query".to_string(),
            required,
            schema: Type::Primitive(Prim::String),
            default: None,
            style: None,
            explode: None,
            allow_reserved: false,
            openapi_content: None,
            openapi_fields: Vec::new(),
            provenance: span("handlers.rs"),
        }
    }

    fn field(name: &str) -> Field {
        Field {
            json_name: name.to_string(),
            serializer_may_omit: false,
            deserializer_accepts_absent: false,
            deserializer_accepts_null: false,
            serializer_may_emit_null: false,
            validator_requires_presence: false,
            validator_rejects_null: false,
            schema: Type::Primitive(Prim::String),
            description: None,
            example: None,
            meta: FieldMeta::default(),
        }
    }

    fn schema(id: &str, fields: Vec<Field>) -> Schema {
        Schema {
            id: id.to_string(),
            name: id.to_string(),
            body: Type::Object(fields),
            enum_source_order: Vec::new(),
            provenance: span("models.rs"),
        }
    }

    fn request_graph(tags: &[&str], root: &str, schemas: Vec<Schema>) -> ApiGraph {
        let mut operation = operation();
        operation.request_body = Some(SchemaRef {
            ref_id: root.to_string(),
        });
        ApiGraph {
            operations: vec![operation],
            operation_docs: (!tags.is_empty())
                .then(|| policy("listBooks", tags))
                .into_iter()
                .collect(),
            schemas,
            ..ApiGraph::default()
        }
    }

    fn response_graph(schema: Schema) -> ApiGraph {
        let id = schema.id.clone();
        let mut operation = operation();
        operation.responses = vec![Response {
            status: 200,
            body: Some(SchemaRef { ref_id: id }),
            body_kind: "json".to_string(),
            content_type: Some("application/json".to_string()),
            content_types: Vec::new(),
        }];
        ApiGraph {
            operations: vec![operation],
            schemas: vec![schema],
            ..ApiGraph::default()
        }
    }

    #[test]
    fn required_nullability_and_enum_changes_are_directional() {
        let mut request_optional = field("value");
        request_optional.deserializer_accepts_absent = true;
        let mut request_required = request_optional.clone();
        request_required.validator_requires_presence = true;
        let request_base = request_graph(
            &[],
            "Payload::input",
            vec![schema("Payload::input", vec![request_optional])],
        );
        let request_current = request_graph(
            &[],
            "Payload::input",
            vec![schema("Payload::input", vec![request_required])],
        );
        let finding = change(
            &diff_graphs(&request_base, &request_current, &BTreeSet::new()),
            "request.property.required.added",
        )
        .clone();
        assert_eq!(finding.kind, ChangeKind::Breaking);

        let mut response_required = field("value");
        response_required.serializer_may_omit = false;
        let mut response_optional = response_required.clone();
        response_optional.serializer_may_omit = true;
        let response_base = response_graph(schema("Payload::output", vec![response_required]));
        let response_current = response_graph(schema("Payload::output", vec![response_optional]));
        assert_eq!(
            change(
                &diff_graphs(&response_base, &response_current, &BTreeSet::new()),
                "response.property.required.removed",
            )
            .kind,
            ChangeKind::Breaking
        );

        let mut accepts_null = field("value");
        accepts_null.deserializer_accepts_null = true;
        let rejects_null = field("value");
        let request_base = request_graph(
            &[],
            "Nullable::input",
            vec![schema("Nullable::input", vec![accepts_null])],
        );
        let request_current = request_graph(
            &[],
            "Nullable::input",
            vec![schema("Nullable::input", vec![rejects_null])],
        );
        assert_eq!(
            change(
                &diff_graphs(&request_base, &request_current, &BTreeSet::new()),
                "request.property.nullability.removed",
            )
            .kind,
            ChangeKind::Breaking
        );

        let mut emits_null = field("value");
        emits_null.serializer_may_emit_null = true;
        let response_base = response_graph(schema("Nullable::output", vec![field("value")]));
        let response_current = response_graph(schema("Nullable::output", vec![emits_null]));
        assert_eq!(
            change(
                &diff_graphs(&response_base, &response_current, &BTreeSet::new()),
                "response.property.nullability.added",
            )
            .kind,
            ChangeKind::Breaking
        );

        let request_base = request_graph(
            &[],
            "State::input",
            vec![Schema {
                id: "State::input".to_string(),
                name: "State".to_string(),
                body: Type::Enum(vec!["active".to_string(), "paused".to_string()]),
                enum_source_order: Vec::new(),
                provenance: span("models.rs"),
            }],
        );
        let request_current = request_graph(
            &[],
            "State::input",
            vec![Schema {
                id: "State::input".to_string(),
                name: "State".to_string(),
                body: Type::Enum(vec!["active".to_string()]),
                enum_source_order: Vec::new(),
                provenance: span("models.rs"),
            }],
        );
        assert_eq!(
            change(
                &diff_graphs(&request_base, &request_current, &BTreeSet::new()),
                "request.enum.value.removed",
            )
            .kind,
            ChangeKind::Breaking
        );

        let response_base = response_graph(Schema {
            id: "State::output".to_string(),
            name: "State".to_string(),
            body: Type::Enum(vec!["active".to_string()]),
            enum_source_order: Vec::new(),
            provenance: span("models.rs"),
        });
        let response_current = response_graph(Schema {
            id: "State::output".to_string(),
            name: "State".to_string(),
            body: Type::Enum(vec!["active".to_string(), "paused".to_string()]),
            enum_source_order: Vec::new(),
            provenance: span("models.rs"),
        });
        assert_eq!(
            change(
                &diff_graphs(&response_base, &response_current, &BTreeSet::new()),
                "response.enum.value.added",
            )
            .kind,
            ChangeKind::Breaking
        );
    }

    #[test]
    fn report_and_json_are_deterministically_sorted() {
        let base = graph_with_tags(&["books"]);
        let mut current = graph_with_tags(&["reports"]);
        current.operations[0].params = vec![parameter(true)];
        let report = diff_graphs(
            &base,
            &current,
            &exemptions(&["partner", "internal", "partner"]),
        );
        assert_eq!(report.policy.exempt_tags, ["internal", "partner"]);
        let first = serde_json::to_string_pretty(&report).expect("serialize report");
        let second = serde_json::to_string_pretty(&report).expect("serialize report again");
        assert_eq!(first, second);
        assert!(report
            .changes
            .windows(2)
            .all(|pair| pair[0].kind <= pair[1].kind));
    }

    #[test]
    fn schema_gate_uses_transitive_most_checked_consumer_and_safe_defaults() {
        let leaf_base = schema("Leaf::input", vec![field("value")]);
        let leaf_current = schema("Leaf::input", Vec::new());
        let root = Schema {
            id: "Root::input".to_string(),
            name: "Root::input".to_string(),
            body: Type::Named("Leaf::input".to_string()),
            enum_source_order: Vec::new(),
            provenance: span("models.rs"),
        };
        let exempt = exemptions(&["internal"]);

        let exempt_base = request_graph(
            &["internal"],
            "Root::input",
            vec![root.clone(), leaf_base.clone()],
        );
        let exempt_current = request_graph(
            &["internal"],
            "Root::input",
            vec![root.clone(), leaf_current.clone()],
        );
        let report = diff_graphs(&exempt_base, &exempt_current, &exempt);
        assert!(!change(&report, "request.property.removed").gating);

        let mut shared_base = exempt_base.clone();
        let mut checked_operation = operation();
        checked_operation.id = "checked".to_string();
        checked_operation.path = "/checked".to_string();
        checked_operation.request_body = Some(SchemaRef {
            ref_id: "Root::input".to_string(),
        });
        shared_base.operations.push(checked_operation.clone());
        let mut shared_current = exempt_current.clone();
        shared_current.operations.push(checked_operation);
        let report = diff_graphs(&shared_base, &shared_current, &exempt);
        assert!(change(&report, "request.property.removed").gating);

        let non_http_base = ApiGraph {
            schemas: vec![leaf_base.clone()],
            schema_uses: vec![SchemaUseRoot {
                schema_id: "Leaf::input".to_string(),
                use_: SchemaUse::Input,
            }],
            ..ApiGraph::default()
        };
        let non_http_current = ApiGraph {
            schemas: vec![leaf_current.clone()],
            schema_uses: non_http_base.schema_uses.clone(),
            ..ApiGraph::default()
        };
        assert!(
            change(
                &diff_graphs(&non_http_base, &non_http_current, &exempt),
                "request.property.removed"
            )
            .gating
        );

        let unused_base = ApiGraph {
            schemas: vec![leaf_base],
            ..ApiGraph::default()
        };
        let unused_current = ApiGraph {
            schemas: vec![leaf_current],
            ..ApiGraph::default()
        };
        assert!(
            change(
                &diff_graphs(&unused_base, &unused_current, &exempt),
                "schema.property.removed"
            )
            .gating
        );
    }

    #[test]
    fn checked_on_either_schema_side_gates() {
        let base = request_graph(
            &["books"],
            "Thing::input",
            vec![schema("Thing::input", vec![field("value")])],
        );
        let current = request_graph(
            &["internal"],
            "Thing::input",
            vec![schema("Thing::input", Vec::new())],
        );
        let report = diff_graphs(&base, &current, &exemptions(&["internal"]));
        assert!(change(&report, "request.property.removed").gating);
    }

    #[test]
    fn taxonomy_covers_operations_http_shapes_schemas_security_and_docs() {
        let mut base_operation = operation();
        base_operation.id = "oldName".to_string();
        base_operation.group = Some("OldGroup".to_string());
        base_operation.params = vec![parameter(false)];
        base_operation.request_body = Some(SchemaRef {
            ref_id: "OldBody".to_string(),
        });
        base_operation.request_body_content_type = Some("application/json".to_string());
        base_operation.responses = vec![Response {
            status: 200,
            body: Some(SchemaRef {
                ref_id: "OldResponse".to_string(),
            }),
            body_kind: "json".to_string(),
            content_type: None,
            content_types: vec!["application/json".to_string()],
        }];

        let mut current_operation = operation();
        current_operation.id = "newName".to_string();
        current_operation.group = Some("NewGroup".to_string());
        current_operation.params = vec![Param {
            name: "token".to_string(),
            ..parameter(true)
        }];
        current_operation.request_body = Some(SchemaRef {
            ref_id: "NewBody".to_string(),
        });
        current_operation.request_body_content_type = Some("application/cbor".to_string());
        current_operation.responses = vec![Response {
            status: 201,
            body: None,
            body_kind: "empty".to_string(),
            content_type: None,
            content_types: Vec::new(),
        }];

        let base = ApiGraph {
            title: "Old".to_string(),
            operations: vec![base_operation],
            schemas: vec![
                schema("Shared", vec![field("value")]),
                schema("Removed", Vec::new()),
            ],
            ..ApiGraph::default()
        };
        let current = ApiGraph {
            title: "New".to_string(),
            operations: vec![current_operation],
            schemas: vec![schema("Shared", Vec::new()), schema("Added", Vec::new())],
            security: vec![SecurityScheme {
                id: "Bearer".to_string(),
                kind: "http".to_string(),
                location: String::new(),
                name: "bearer".to_string(),
                global: true,
            }],
            ..ApiGraph::default()
        };
        let report = diff_graphs(&base, &current, &BTreeSet::new());
        let codes: BTreeSet<&str> = report
            .changes
            .iter()
            .map(|change| change.code.as_str())
            .collect();
        for expected in [
            "document.title.changed",
            "operation.name.changed",
            "sdk.group.changed",
            "request.parameter.removed",
            "request.parameter.required.added",
            "request.body.schema.changed",
            "request.body.media_type.removed",
            "request.body.media_type.added",
            "response.status.removed",
            "response.status.added",
            "schema.property.removed",
            "schema.removed",
            "schema.added",
            "security.scheme.added",
            "security.operation.added",
        ] {
            assert!(codes.contains(expected), "missing {expected}: {codes:?}");
        }
    }

    #[test]
    fn method_or_path_change_has_explicit_taxonomy() {
        let base = graph_with_tags(&[]);
        let mut current = graph_with_tags(&[]);
        current.operations[0].method = "POST".to_string();
        current.operations[0].path = "/volumes".to_string();
        let report = diff_graphs(&base, &current, &BTreeSet::new());
        assert_eq!(
            change(&report, "operation.method.changed").kind,
            ChangeKind::Breaking
        );
        assert_eq!(
            change(&report, "operation.path.changed").kind,
            ChangeKind::Breaking
        );
        assert_eq!(report.changes.len(), 2);
    }

    #[test]
    fn response_status_is_additive_but_body_shape_addition_is_breaking() {
        let mut base = graph_with_tags(&[]);
        let mut current = base.clone();
        current.operations[0].responses.push(Response {
            status: 200,
            body: Some(SchemaRef {
                ref_id: "Book".to_string(),
            }),
            body_kind: "json".to_string(),
            content_type: Some("application/json".to_string()),
            content_types: Vec::new(),
        });
        assert_eq!(
            change(
                &diff_graphs(&base, &current, &BTreeSet::new()),
                "response.status.added"
            )
            .kind,
            ChangeKind::Additive
        );

        base.operations[0].responses = vec![Response {
            status: 204,
            body: None,
            body_kind: "empty".to_string(),
            content_type: None,
            content_types: Vec::new(),
        }];
        current = base.clone();
        current.operations[0].responses[0].body = Some(SchemaRef {
            ref_id: "Book".to_string(),
        });
        assert_eq!(
            change(
                &diff_graphs(&base, &current, &BTreeSet::new()),
                "response.body.added"
            )
            .kind,
            ChangeKind::Breaking
        );
    }

    #[test]
    fn unconsumed_schema_axis_changes_are_breaking_for_generated_models() {
        let mut optional = field("value");
        optional.deserializer_accepts_absent = true;
        let required = field("value");
        let base = ApiGraph {
            schemas: vec![schema("Standalone", vec![required.clone()])],
            ..ApiGraph::default()
        };
        let current = ApiGraph {
            schemas: vec![schema("Standalone", vec![optional])],
            ..ApiGraph::default()
        };
        assert_eq!(
            change(
                &diff_graphs(&base, &current, &BTreeSet::new()),
                "schema.property.required.removed"
            )
            .kind,
            ChangeKind::Breaking
        );

        let mut nullable = required.clone();
        nullable.deserializer_accepts_null = true;
        let current = ApiGraph {
            schemas: vec![schema("Standalone", vec![nullable])],
            ..ApiGraph::default()
        };
        assert_eq!(
            change(
                &diff_graphs(&base, &current, &BTreeSet::new()),
                "schema.property.nullability.added"
            )
            .kind,
            ChangeKind::Breaking
        );
    }

    #[test]
    fn operation_security_distinguishes_looser_and_stricter_alternatives() {
        let mut base = graph_with_tags(&[]);
        base.operations[0].security_overrides_global = true;
        base.operations[0].security = vec!["Bearer".to_string(), "Key".to_string()];
        let mut looser = base.clone();
        looser.operations[0].security = vec!["Bearer".to_string()];
        assert_eq!(
            change(
                &diff_graphs(&base, &looser, &BTreeSet::new()),
                "security.operation.changed"
            )
            .kind,
            ChangeKind::Additive
        );
        assert_eq!(
            change(
                &diff_graphs(&looser, &base, &BTreeSet::new()),
                "security.operation.changed"
            )
            .kind,
            ChangeKind::Breaking
        );
    }

    #[test]
    fn parameter_documentation_is_doc_only_and_uses_parameter_location() {
        let mut base = graph_with_tags(&[]);
        base.operations[0].params.push(parameter(false));
        let mut current = base.clone();
        current.operations[0].params[0].provenance = span("query.rs");
        current.operations[0].params[0]
            .openapi_fields
            .push(("description".to_string(), serde_json::json!("Page size")));
        let report = diff_graphs(&base, &current, &BTreeSet::new());
        let finding = change(&report, "request.parameter.documentation.changed");
        assert_eq!(finding.kind, ChangeKind::DocOnly);
        assert_eq!(finding.file.as_deref(), Some("query.rs"));

        let removed = diff_graphs(&base, &graph_with_tags(&[]), &BTreeSet::new());
        assert!(change(&removed, "request.parameter.removed").span.is_none());
    }

    #[test]
    fn enum_declaration_order_is_reported_without_value_churn() {
        let base = ApiGraph {
            schemas: vec![Schema {
                id: "State".to_string(),
                name: "State".to_string(),
                body: Type::Enum(vec!["active".to_string(), "paused".to_string()]),
                enum_source_order: Vec::new(),
                provenance: span("models.rs"),
            }],
            ..ApiGraph::default()
        };
        let mut current = base.clone();
        current.schemas[0].body = Type::Enum(vec!["paused".to_string(), "active".to_string()]);
        let report = diff_graphs(&base, &current, &BTreeSet::new());
        assert_eq!(report.changes.len(), 1);
        assert_eq!(report.changes[0].code, "schema.enum.order.changed");
        assert_eq!(report.changes[0].kind, ChangeKind::DocOnly);
    }
}
