//! Diagnostics seam (Phase 2+): collects warnings for unsupported / lossy source patterns (D-10).
//!
//! [`collect`] runs the `goextract` helper, takes the diagnostics it emits (each a severity + a
//! machine-stable rule/identity message + a `file:line`), and renders them to a canonical text block.
//! The rendered text reconciles with `fixtures/goalservice/expected/diagnostics.txt`: 7 `WARN` lines
//! — float64-narrowing ×3, free-form-map ×1, untyped-query ×3 — in a stable, reviewable order.
//!
//! Canonical phrasing (Open Q2 / RESEARCH Pitfall 4): the helper normalizes each rule to ONE template
//! carrying the field-or-route identity, so the three float64 lines share trailing phrasing rather
//! than the slightly-divergent wording in the hand-authored `expected/diagnostics.txt`. We render each
//! diagnostic as `WARN  <message> (<file>:<line>)` (severity + identity + source location, D-10),
//! relativizing the file path against the analyzed module so the output is portable + byte-stable.
//! The `snapshot_diagnostics` test locks the exact final text.

use crate::analyze::{facts::DiagnosticFact, helper};

/// Collect diagnostics for a Go fixture/module directory and render them as canonical text (D-10).
///
/// Runs the helper, sorts the diagnostics by `(file, line, message)` for a stable, reviewable order,
/// and renders one `WARN  <message> (<file>:<line>)` line each. File paths are relativized against
/// `fixture_dir` so the text is identical across machines (GRAPH-02 discipline).
///
/// # Errors
///
/// Propagates the typed subprocess errors from [`helper::run_goextract`] — never a panic (GO-06):
/// - [`crate::CoreError::GoToolchainMissing`] if `go` cannot be spawned.
/// - [`crate::CoreError::HelperExit`] if the helper exits non-zero.
/// - [`crate::CoreError::FactsParse`] if the helper's stdout is not the expected JSON.
pub fn collect(fixture_dir: &str) -> Result<String, crate::CoreError> {
    // Resolve to an absolute target so a relative `fixture_dir` works (the helper runs from the
    // goextract dir) AND diagnostic file paths relativize against the same root the helper saw.
    let target = helper::resolve_target(fixture_dir);
    let facts = helper::run_goextract(&target)?;
    Ok(render(facts.diagnostics, &target))
}

/// Render a diagnostics list to the canonical text block (pure; testable without the subprocess).
fn render(mut diagnostics: Vec<DiagnosticFact>, module_root: &str) -> String {
    let root = module_root.trim_end_matches('/');
    diagnostics.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.message.cmp(&b.message))
    });
    diagnostics
        .into_iter()
        .map(|diag| {
            let file = relativize(&diag.file, root);
            format!(
                "{}  {} ({}:{})",
                diag.severity, diag.message, file, diag.line
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Strip the analyzed-module prefix from a diagnostic file path so output is portable + byte-stable.
fn relativize(file: &str, root: &str) -> String {
    if root.is_empty() {
        return file.to_string();
    }
    if let Some(rest) = file.strip_prefix(root) {
        return rest.trim_start_matches('/').to_string();
    }
    file.to_string()
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow
    // to the test module so the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::render;
    use crate::analyze::facts::DiagnosticFact;

    fn warn(message: &str, file: &str, line: u32) -> DiagnosticFact {
        DiagnosticFact {
            severity: "WARN".to_string(),
            message: message.to_string(),
            file: file.to_string(),
            line,
        }
    }

    #[test]
    fn renders_severity_message_and_relative_location_per_line() {
        let out = render(
            vec![warn(
                "free-form map field: GoalResponse.Metadata (map[string]any) lowers to additionalProperties: true",
                "/root/internal/common/dto/goal.go",
                62,
            )],
            "/root",
        );
        assert_eq!(
            out,
            "WARN  free-form map field: GoalResponse.Metadata (map[string]any) lowers to additionalProperties: true (internal/common/dto/goal.go:62)"
        );
    }

    #[test]
    fn sorts_by_file_then_line_for_a_stable_reviewable_order() {
        // Intentionally out of order: handlers.go before goal.go, lines descending.
        let out = render(
            vec![
                warn("untyped query param 'aggregation'", "/root/handlers.go", 59),
                warn(
                    "float64 narrowing GoalResponse.TargetValue",
                    "/root/goal.go",
                    57,
                ),
                warn("untyped query param 'cursor'", "/root/handlers.go", 57),
            ],
            "/root",
        );
        let lines: Vec<&str> = out.lines().collect();
        // goal.go sorts before handlers.go; within handlers.go line 57 before 59.
        assert!(lines[0].contains("goal.go:57"), "{out}");
        assert!(lines[1].contains("handlers.go:57"), "{out}");
        assert!(lines[2].contains("handlers.go:59"), "{out}");
    }

    #[test]
    fn render_is_byte_identical_across_two_runs() {
        let diags = vec![warn("b", "/root/x.go", 2), warn("a", "/root/x.go", 1)];
        let a = render(diags.clone_for_test(), "/root");
        let b = render(diags, "/root");
        assert_eq!(a, b);
    }

    // Small test helper: DiagnosticFact is not Clone in production (it is a deserialize-only DTO), so
    // clone it field-by-field for the determinism test without widening the production surface.
    trait CloneForTest {
        fn clone_for_test(&self) -> Vec<DiagnosticFact>;
    }
    impl CloneForTest for Vec<DiagnosticFact> {
        fn clone_for_test(&self) -> Vec<DiagnosticFact> {
            self.iter()
                .map(|d| warn(&d.message, &d.file, d.line))
                .collect()
        }
    }
}
