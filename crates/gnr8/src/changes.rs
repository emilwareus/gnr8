//! Human and machine presentation for `gnr8 changes`.

use std::fmt::Write as _;

use gnr8_engine::changes::{BaseGraph, Change, ChangeKind, ChangeReport};

#[derive(serde::Serialize)]
struct BaseRevision<'a> {
    #[serde(rename = "ref")]
    reference: &'a str,
    resolved: &'a str,
}

#[derive(serde::Serialize)]
struct MachineReport<'a> {
    base: BaseRevision<'a>,
    policy: &'a gnr8_engine::changes::ChangePolicy,
    summary: &'a gnr8_engine::changes::ChangeSummary,
    changes: &'a [Change],
}

/// Render the stable JSON report, including the requested and resolved base revision.
pub(crate) fn render_json(
    base: &BaseGraph,
    report: &ChangeReport,
) -> Result<String, serde_json::Error> {
    let output = MachineReport {
        base: BaseRevision {
            reference: &base.reference,
            resolved: &base.commit,
        },
        policy: &report.policy,
        summary: &report.summary,
        changes: &report.changes,
    };
    let mut text = serde_json::to_string_pretty(&output)?;
    text.push('\n');
    Ok(text)
}

/// Render findings in the issue #75 three-column format.
pub(crate) fn render_human(report: &ChangeReport) -> String {
    let mut text = String::new();
    if !report.policy.exempt_tags.is_empty() {
        let _ = writeln!(
            text,
            "changes: exempt tags: {}",
            report.policy.exempt_tags.join(", ")
        );
    }
    if report.changes.is_empty() {
        text.push_str("No API changes.\n");
        return text;
    }
    for change in &report.changes {
        let operation = change.operation.as_deref().unwrap_or("-");
        let suffix = exemption_suffix(change);
        let _ = writeln!(
            text,
            "{:<9} {:<19} {}{}",
            kind_label(change.kind),
            operation,
            change.message,
            suffix
        );
    }
    text
}

const fn kind_label(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Breaking => "BREAKING",
        ChangeKind::Additive => "ADDITIVE",
        ChangeKind::DocOnly => "DOC-ONLY",
    }
}

fn exemption_suffix(change: &Change) -> &'static str {
    if change.kind != ChangeKind::Breaking || change.gating {
        return "";
    }
    match (change.exempt.base, change.exempt.current) {
        (Some(true), Some(true)) => "  (exempt on both sides; not gating)",
        (Some(true), None) => "  (exempt on base side; not gating)",
        (None, Some(true)) => "  (exempt on current side; not gating)",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use gnr8_engine::changes::{
        AffectedOperation, BaseGraph, Change, ChangeKind, ChangePolicy, ChangeReport,
        ChangeSummary, Sides,
    };

    use super::{render_human, render_json};

    fn finding(kind: ChangeKind, gating: bool, exempt: Sides<bool>) -> Change {
        Change {
            kind,
            code: "operation.removed".to_string(),
            operation: Some("DELETE /books/{id}".to_string()),
            operation_id: Some("deleteBook".to_string()),
            subject: None,
            affected_operations: Sides {
                base: Some(vec![AffectedOperation {
                    operation: "DELETE /books/{id}".to_string(),
                    operation_id: "deleteBook".to_string(),
                }]),
                current: None,
            },
            tags: Sides {
                base: Some(vec!["internal".to_string()]),
                current: None,
            },
            exempt,
            gating,
            message: "operation removed".to_string(),
            file: None,
            line: None,
            span: None,
        }
    }

    #[test]
    fn human_report_uses_three_columns_and_side_accurate_suffixes() {
        let report = ChangeReport {
            policy: ChangePolicy {
                exempt_tags: vec!["internal".to_string()],
            },
            summary: ChangeSummary {
                breaking: 2,
                additive: 0,
                doc_only: 0,
                gating: 1,
            },
            changes: vec![
                finding(
                    ChangeKind::Breaking,
                    true,
                    Sides {
                        base: Some(false),
                        current: None,
                    },
                ),
                finding(
                    ChangeKind::Breaking,
                    false,
                    Sides {
                        base: Some(true),
                        current: None,
                    },
                ),
            ],
        };
        let rendered = render_human(&report);
        assert!(rendered.contains("BREAKING  DELETE /books/{id}  operation removed\n"));
        assert!(rendered.contains("(exempt on base side; not gating)"));
        assert!(rendered.starts_with("changes: exempt tags: internal\n"));
    }

    #[test]
    fn json_report_carries_base_policy_sides_and_summary() {
        let base = BaseGraph {
            reference: "origin/main".to_string(),
            commit: "0123456789012345678901234567890123456789".to_string(),
            graph: gnr8_engine::graph::ApiGraph::default(),
        };
        let report = ChangeReport {
            policy: ChangePolicy {
                exempt_tags: vec!["internal".to_string()],
            },
            summary: ChangeSummary {
                breaking: 1,
                additive: 0,
                doc_only: 0,
                gating: 0,
            },
            changes: vec![finding(
                ChangeKind::Breaking,
                false,
                Sides {
                    base: Some(true),
                    current: None,
                },
            )],
        };
        let value: serde_json::Value =
            serde_json::from_str(&render_json(&base, &report).expect("render JSON"))
                .expect("parse JSON");
        assert_eq!(value["base"]["ref"], "origin/main");
        assert_eq!(value["policy"]["exempt_tags"][0], "internal");
        assert_eq!(value["changes"][0]["exempt"]["base"], true);
        assert_eq!(
            value["changes"][0]["affected_operations"]["base"][0]["operation_id"],
            "deleteBook"
        );
        assert_eq!(value["summary"]["gating"], 0);
    }

    #[test]
    fn empty_human_report_is_explicit() {
        let report = ChangeReport {
            policy: ChangePolicy {
                exempt_tags: Vec::new(),
            },
            summary: ChangeSummary::default(),
            changes: Vec::new(),
        };
        assert_eq!(render_human(&report), "No API changes.\n");
    }
}
