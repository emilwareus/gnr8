//! `gnr8 doctor` — the health aggregator (HARD-01 / D-01, D-02).
//!
//! This module owns the PURE report shape + grouping + exit-policy decision + human/JSON render. It
//! performs NO I/O: [`DoctorReport::assemble`] takes already-collected facts (the lifecycle booleans,
//! the pipeline's structured `diagnostics`, and the dry-run drift `WritePlan`) and groups them into a
//! serializable report. The impure half — probing `.gnr8/`, probing the source-language toolchain, and RUNNING the
//! user's `.gnr8/` pipeline (the child) to harvest its diagnostics + compute drift — lives in
//! `main::run_doctor`, mirroring the `run_check` shell-vs-decision split. Keeping `assemble` pure makes
//! the entire exit-policy truth table (Pitfall 1 — informational WARNs must NOT force a non-zero exit)
//! unit-testable without a filesystem, a toolchain, or a child process.
//!
//! ## Exit policy (Pitfall 1 / HARD-01)
//!
//! [`DoctorReport::has_actionable_problem`] is `true` iff a lifecycle/staleness problem exists:
//! `.gnr8/` missing, the source-language toolchain absent, the pipeline failed to run, or any output is stale
//! (`Write`) / drifted (`UserEdited`). The pipeline's analysis `diagnostics` (e.g. the fixture's known
//! unsupported-pattern WARNs) are INFORMATIONAL and are deliberately EXCLUDED — they explain "what I
//! can't represent and why" and must never make `doctor` permanently red. This mirrors `run_check`,
//! which exits non-zero only on `has_drift()`, not on clean-but-present outputs.

// User-facing prose dense with proper nouns/acronyms (OpenAPI, PoC, `.gnr8/`, ...); backticking them
// would hurt readability. Allow `doc_markdown` module-wide (skill ch.2.4; mirrors the scoped allow in
// cli.rs / watch.rs).
#![allow(clippy::doc_markdown)]

use std::fmt::Write as _;

use gnr8::graph::Diagnostic;
use gnr8::lifecycle::{WriteAction, WritePlan};

/// The read-only lifecycle facts `doctor` reports (each is an ACTIONABLE problem when false). Collected
/// by `run_doctor` and handed to [`DoctorReport::assemble`].
///
/// These flags ARE the documented `lifecycle` JSON sub-object (RESEARCH `doctor --json` shape): each is
/// an independent boolean health fact a CI gate reads by name, not a state machine — so the
/// `struct_excessive_bools` lint is allowed locally rather than collapsing them into enums and breaking
/// the published field set.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, serde::Serialize)]
pub(crate) struct LifecycleHealth {
    /// Whether the project-local `.gnr8/` generation crate exists (run `gnr8 init` if not).
    pub(crate) initialized: bool,
    /// Whether the DETECTED source language's toolchain is present (a version probe of the language's
    /// binary — `go`/`python3`/`node` — spawned successfully). Generalized from the old Go-only
    /// boolean (XLANG-04): a FastAPI project probes `python3`, a NestJS project `node`.
    pub(crate) source_toolchain: bool,
    /// Which source language `doctor` detected and probed (`"go"`/`"python"`/`"typescript"`, or
    /// `"unknown"` when the source dir is empty/ambiguous). Names WHICH toolchain `source_toolchain`
    /// reflects so the report is honest about the multi-language choice (RESEARCH A6).
    pub(crate) language: String,
    /// Whether the user's `.gnr8/` pipeline RAN (compiled + emitted a bundle). False ⇒ a compile error
    /// in the pipeline, a missing toolchain it needs, or a missing `.gnr8/` — all actionable.
    pub(crate) pipeline_runs: bool,
}

/// The stale/drifted/clean partition of the dry-run [`WritePlan`] (mirrors `run_check`'s partition).
#[derive(Debug, serde::Serialize, Default)]
pub(crate) struct OutputHealth {
    /// Output paths that are out of date (`WriteAction::Write`) — run `gnr8 generate` (actionable).
    pub(crate) stale: Vec<String>,
    /// Output paths a human hand-edited (`WriteAction::UserEdited`) — review/regenerate (actionable).
    pub(crate) drifted: Vec<String>,
    /// Output paths byte-identical to the generated output (`WriteAction::Unchanged`) — clean.
    pub(crate) unchanged: Vec<String>,
}

/// One analysis diagnostic enriched with a short, human explanation of WHY it is flagged and HOW to
/// address it (D-02 — `doctor` explains, it does not re-analyze). Carries the structured
/// [`Diagnostic`] fields verbatim plus the derived `why`/`fix` strings.
#[derive(Debug, serde::Serialize)]
pub(crate) struct DoctorDiagnostic {
    /// Stable diagnostic identity.
    pub(crate) code: String,
    /// Severity copied from the source diagnostic (`"WARN"` / `"ERROR"`).
    pub(crate) severity: String,
    /// Stable diagnostic category.
    pub(crate) category: gnr8::graph::DiagnosticCategory,
    /// The original analyzer message (rule + identity).
    pub(crate) message: String,
    /// The source file the diagnostic applies to (module-relative).
    pub(crate) file: String,
    /// The 1-based source line.
    pub(crate) line: u32,
    /// A short explanation of why this pattern is an expected PoC limitation.
    pub(crate) why: String,
    /// A short suggestion for how to address (or live with) the pattern.
    pub(crate) fix: String,
}

/// Counts surfaced in the report header so a reader sees the informational-vs-actionable split at a
/// glance (Pitfall 1 — make the policy visible).
#[derive(Debug, serde::Serialize)]
pub(crate) struct DoctorSummary {
    /// How many ACTIONABLE problems exist (the count `has_actionable_problem` is non-zero for).
    pub(crate) actionable_problems: usize,
    /// How many INFORMATIONAL analysis diagnostics exist (expected PoC limitations).
    pub(crate) informational_diagnostics: usize,
}

/// SDK/OpenAPI readiness for one generated target.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SdkReadiness {
    /// Target language/artifact kind (`"go"`, `"python"`, `"typescript"`, or `"openapi"`).
    pub(crate) language: String,
    /// Project-relative target output path.
    pub(crate) output_path: String,
    /// Required target toolchain/check runner.
    pub(crate) toolchain: String,
    /// Stable status string: `"ready"` or `"not_ready"`.
    pub(crate) status: String,
    /// Empty when ready; actionable failure reason when not ready.
    pub(crate) reason: String,
}

impl SdkReadiness {
    /// Build a ready target entry.
    #[must_use]
    pub(crate) fn ready(
        language: impl Into<String>,
        output_path: impl Into<String>,
        toolchain: impl Into<String>,
    ) -> Self {
        Self {
            language: language.into(),
            output_path: output_path.into(),
            toolchain: toolchain.into(),
            status: "ready".to_string(),
            reason: String::new(),
        }
    }

    /// Build an actionable not-ready target entry.
    #[must_use]
    pub(crate) fn not_ready(
        language: impl Into<String>,
        output_path: impl Into<String>,
        toolchain: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            language: language.into(),
            output_path: output_path.into(),
            toolchain: toolchain.into(),
            status: "not_ready".to_string(),
            reason: reason.into(),
        }
    }

    fn is_ready(&self) -> bool {
        self.status == "ready"
    }
}

/// Runtime facts collected by the impure doctor shell.
#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct DoctorRuntime {
    /// Current gnr8 binary path.
    pub(crate) binary_path: Option<String>,
    /// Resolved resource directory, if any.
    pub(crate) resource_dir: Option<String>,
    /// Output anchors reported by the generation pipeline.
    pub(crate) output_anchors: Vec<String>,
}

/// Doctor timings in milliseconds.
#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct DoctorTimings {
    /// Total command runtime.
    pub(crate) total: u128,
}

/// The full `gnr8 doctor` report: lifecycle health, output staleness, analysis diagnostics, and a
/// header summary. `healthy` is the top-level boolean a CI gate reads (== `!has_actionable_problem()`).
#[derive(Debug, serde::Serialize)]
pub(crate) struct DoctorReport {
    /// The top-level CI signal: `true` iff no actionable problem exists.
    pub(crate) healthy: bool,
    /// The lifecycle facts (init / source-language toolchain + language / pipeline runs).
    pub(crate) lifecycle: LifecycleHealth,
    /// The stale/drifted/clean output partition (empty when the pipeline did not run).
    pub(crate) outputs: OutputHealth,
    /// The informational analysis diagnostics (the unsupported patterns the pipeline reported).
    pub(crate) diagnostics: Vec<DoctorDiagnostic>,
    /// Per generated SDK/OpenAPI target readiness checks.
    pub(crate) sdk_readiness: Vec<SdkReadiness>,
    /// The informational-vs-actionable header counts.
    pub(crate) summary: DoctorSummary,
    /// Runtime resolution facts.
    pub(crate) runtime: DoctorRuntime,
    /// Timings in milliseconds.
    pub(crate) timings_ms: DoctorTimings,
}

/// Map a structured analysis [`Diagnostic`] to a short (why, fix) explanation by matching the message
/// kind (free-form map, float64 narrowing, untyped query param). Defaults to a generic explanation so
/// an unrecognized future diagnostic still renders sensibly. PURE string classification — no I/O.
fn explain(message: &str) -> (String, String) {
    if message.contains("map[string]any") || message.contains("free-form map") {
        (
            "a free-form map lowers to `additionalProperties: true`, so the value shape is unconstrained"
                .to_string(),
            "model the map as a typed struct if the keys/values are known, or accept the open shape"
                .to_string(),
        )
    } else if message.contains("narrowing") || message.contains("float64") {
        (
            "Go `float64` is narrowed in the generated SDK and loses precision".to_string(),
            "keep the field as `float64` end-to-end, or accept the documented precision trade-off"
                .to_string(),
        )
    } else if message.contains("query param") {
        (
            "an untyped query parameter is recovered as a bare string; its type/required-ness is under-specified"
                .to_string(),
            "bind the parameter via a typed struct so the analyzer can recover its real type"
                .to_string(),
        )
    } else {
        (
            "an unsupported source pattern the analyzer cannot fully represent (expected PoC limitation)"
                .to_string(),
            "see the diagnostic message and the SDK docs for the supported alternative".to_string(),
        )
    }
}

impl DoctorReport {
    /// Build a grouped [`DoctorReport`] from already-collected facts (PURE — no I/O, no analysis).
    ///
    /// - `initialized` — `.gnr8/` exists.
    /// - `source_present` — the DETECTED source language's toolchain probe result.
    /// - `language` — the detected source language label (`"go"`/`"python"`/`"typescript"`/`"unknown"`).
    /// - `pipeline_ran` — the user's `.gnr8/` pipeline compiled + emitted a bundle.
    /// - `diagnostics` — the pipeline's structured analysis diagnostics, or `None` when the pipeline did
    ///   not run (degrades gracefully, never a crash).
    /// - `drift` — the dry-run [`WritePlan`] computed from the pipeline's artifacts, or `None` when the
    ///   pipeline did not run (the `pipeline_runs = false` finding carries the verdict then).
    ///
    /// The `WritePlan` is partitioned into stale (`Write`) / drifted (`UserEdited`) / clean
    /// (`Unchanged`), and each `Diagnostic` is enriched with a why/fix explanation.
    //
    // Each bool here is an INDEPENDENT collected health fact the impure `run_doctor` shell hands in by
    // name (mirrors the `struct_excessive_bools` allow on `LifecycleHealth`); allow the param-bools lint
    // locally rather than restructure (skill ch.2.4).
    #[allow(clippy::fn_params_excessive_bools)]
    pub(crate) fn assemble(
        initialized: bool,
        source_present: bool,
        language: &str,
        pipeline_ran: bool,
        diagnostics: Option<Vec<Diagnostic>>,
        drift: Option<&WritePlan>,
    ) -> Self {
        let lifecycle = LifecycleHealth {
            initialized,
            source_toolchain: source_present,
            language: language.to_string(),
            pipeline_runs: pipeline_ran,
        };

        // Partition the dry-run plan exactly as `run_check` does (stale / drifted / clean). When the
        // pipeline did not run, the partition stays empty and the `pipeline_runs = false` finding (not a
        // false-healthy "all up to date") carries the actionable verdict.
        let mut outputs = OutputHealth::default();
        if let Some(plan) = drift {
            for file in &plan.files {
                match file.action {
                    WriteAction::Write => outputs.stale.push(file.path.clone()),
                    WriteAction::UserEdited => outputs.drifted.push(file.path.clone()),
                    WriteAction::Unchanged => outputs.unchanged.push(file.path.clone()),
                }
            }
        }

        // Enrich each informational analysis diagnostic with a short why/fix (D-02 — explain, not
        // re-analyze). `None` (pipeline did not run) yields an empty list, not an error.
        let diagnostics: Vec<DoctorDiagnostic> = diagnostics
            .unwrap_or_default()
            .into_iter()
            .map(|d| {
                let (why, fix) = explain(&d.message);
                DoctorDiagnostic {
                    code: d.code,
                    severity: d.severity,
                    category: d.category,
                    message: d.message,
                    file: d.file,
                    line: d.line,
                    why,
                    fix,
                }
            })
            .collect();

        // Assemble in two phases so the summary/`healthy` fields can be derived from the populated
        // lifecycle/outputs/diagnostics groups without re-deriving them by hand.
        let mut report = Self {
            healthy: true, // provisional — overwritten below from `has_actionable_problem`.
            lifecycle,
            outputs,
            diagnostics,
            sdk_readiness: Vec::new(),
            summary: DoctorSummary {
                actionable_problems: 0,
                informational_diagnostics: 0,
            },
            runtime: DoctorRuntime::default(),
            timings_ms: DoctorTimings::default(),
        };
        let actionable = report.actionable_problem_count();
        report.summary = DoctorSummary {
            actionable_problems: actionable,
            informational_diagnostics: report.diagnostics.len(),
        };
        report.healthy = !report.has_actionable_problem();
        report
    }

    /// Attach SDK/OpenAPI readiness facts collected by the `run_doctor` shell.
    #[must_use]
    pub(crate) fn with_sdk_readiness(mut self, sdk_readiness: Vec<SdkReadiness>) -> Self {
        self.sdk_readiness = sdk_readiness;
        let actionable = self.actionable_problem_count();
        self.summary.actionable_problems = actionable;
        self.healthy = !self.has_actionable_problem();
        self
    }

    /// Attach runtime/timing facts collected by the `run_doctor` shell.
    #[must_use]
    pub(crate) fn with_runtime(
        mut self,
        runtime: DoctorRuntime,
        timings_ms: DoctorTimings,
    ) -> Self {
        self.runtime = runtime;
        self.timings_ms = timings_ms;
        self
    }

    /// The number of ACTIONABLE problems (each contributing lifecycle/staleness issue counts once;
    /// every stale and every drifted output counts individually). Analysis diagnostics are EXCLUDED
    /// (Pitfall 1).
    fn actionable_problem_count(&self) -> usize {
        let mut count = 0;
        if !self.lifecycle.initialized {
            count += 1;
        }
        if !self.lifecycle.source_toolchain {
            count += 1;
        }
        if !self.lifecycle.pipeline_runs {
            count += 1;
        }
        count += self.outputs.stale.len();
        count += self.outputs.drifted.len();
        count += self
            .sdk_readiness
            .iter()
            .filter(|readiness| !readiness.is_ready())
            .count();
        count
    }

    /// Whether `doctor` should exit non-zero: `true` iff an ACTIONABLE lifecycle/staleness problem
    /// exists. The informational analysis `diagnostics` are EXCLUDED so the expected unsupported-pattern
    /// WARNs never make `doctor` permanently red (Pitfall 1 / the exit-code contract in RESEARCH).
    /// Mirrors `run_check`'s `has_drift()`-only exit policy.
    #[must_use]
    pub(crate) fn has_actionable_problem(&self) -> bool {
        !self.lifecycle.initialized
            || !self.lifecycle.source_toolchain
            || !self.lifecycle.pipeline_runs
            || !self.outputs.stale.is_empty()
            || !self.outputs.drifted.is_empty()
            || self
                .sdk_readiness
                .iter()
                .any(|readiness| !readiness.is_ready())
    }

    /// Render the grouped human report (LIFECYCLE / OUTPUTS / DIAGNOSTICS) into a String, mirroring the
    /// `writeln!`-into-a-String style of `render.rs`. The trailing line summarizes the actionable
    /// count or reports "healthy"; the diagnostics group is explicitly labeled informational so a
    /// reader never mistakes the expected PoC WARNs for failures (Pitfall 1).
    #[must_use]
    pub(crate) fn render_human(&self) -> String {
        let mut out = String::new();

        // LIFECYCLE group — each fact rendered as OK / a problem line.
        let _ = writeln!(out, "LIFECYCLE");
        let _ = writeln!(
            out,
            "  .gnr8/ crate:        {}",
            ok_or(self.lifecycle.initialized, "missing — run `gnr8 init`")
        );
        let _ = writeln!(
            out,
            "  Source toolchain ({}): {}",
            self.lifecycle.language,
            ok_or(
                self.lifecycle.source_toolchain,
                "NOT FOUND — install the source language's toolchain and ensure it is on PATH"
            )
        );
        let _ = writeln!(
            out,
            "  pipeline runs:       {}",
            ok_or(
                self.lifecycle.pipeline_runs,
                "FAILED — `gnr8 generate` for the compile/run error"
            )
        );

        // OUTPUTS group — the stale/drifted/clean partition.
        let _ = writeln!(
            out,
            "\nOUTPUTS ({} stale, {} drifted, {} unchanged)",
            self.outputs.stale.len(),
            self.outputs.drifted.len(),
            self.outputs.unchanged.len()
        );
        for path in &self.outputs.stale {
            let _ = writeln!(out, "  stale:    {path} (run `gnr8 generate`)");
        }
        for path in &self.outputs.drifted {
            let _ = writeln!(
                out,
                "  drifted:  {path} (hand-edited; differs from generated)"
            );
        }
        if !self.lifecycle.pipeline_runs {
            // The pipeline did not run, so staleness is UNVERIFIED — never imply healthy here.
            let _ = writeln!(
                out,
                "  drift: UNKNOWN — the pipeline did not run (see the LIFECYCLE finding)"
            );
        } else if self.outputs.stale.is_empty() && self.outputs.drifted.is_empty() {
            let _ = writeln!(out, "  (all outputs up to date)");
        }

        render_sdk_readiness(&mut out, &self.sdk_readiness);

        // DIAGNOSTICS group — explicitly labeled informational so the expected PoC WARNs are not
        // mistaken for failures.
        let _ = writeln!(
            out,
            "\nDIAGNOSTICS ({} informational — expected PoC limitations)",
            self.diagnostics.len()
        );
        if self.diagnostics.is_empty() {
            let _ = writeln!(out, "  (none)");
        }
        for d in &self.diagnostics {
            let _ = writeln!(
                out,
                "  {}  [{}] {} ({}:{})",
                d.severity, d.code, d.message, d.file, d.line
            );
            let _ = writeln!(out, "        why: {}", d.why);
            let _ = writeln!(out, "        fix: {}", d.fix);
        }

        // Trailing verdict line.
        let actionable = self.summary.actionable_problems;
        if actionable == 0 {
            let _ = writeln!(
                out,
                "\nhealthy — 0 actionable problems ({} informational diagnostic(s))",
                self.summary.informational_diagnostics
            );
        } else {
            let _ = writeln!(
                out,
                "\n{actionable} actionable problem(s) found ({} informational diagnostic(s))",
                self.summary.informational_diagnostics
            );
        }
        out
    }
}

fn render_sdk_readiness(out: &mut String, readiness: &[SdkReadiness]) {
    let _ = writeln!(out, "\nSDK READINESS ({} target(s))", readiness.len());
    if readiness.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for entry in readiness {
        if entry.is_ready() {
            let _ = writeln!(
                out,
                "  {} {}: ready ({})",
                entry.language, entry.output_path, entry.toolchain
            );
        } else {
            let _ = writeln!(
                out,
                "  {} {}: NOT READY ({}) — {}",
                entry.language, entry.output_path, entry.toolchain, entry.reason
            );
        }
    }
}

/// Render a boolean health fact as `OK` or a problem label (the LIFECYCLE group cell formatter).
fn ok_or(is_ok: bool, problem: &str) -> String {
    if is_ok {
        "OK".to_string()
    } else {
        problem.to_string()
    }
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4); scope the allow to
    // this module so the workspace-wide RUST-04 deny stays intact for production code (mirrors cli.rs /
    // watch.rs test modules).
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::DoctorReport;
    use gnr8::graph::{Diagnostic, DiagnosticCategory, SourceSpan};
    use gnr8::lifecycle::{PlannedFile, WriteAction, WritePlan};
    use std::collections::HashSet;

    /// One informational analysis WARN (the kind the fixture emits several of).
    fn warn_diag(message: &str) -> Diagnostic {
        Diagnostic::new(
            "source.test",
            DiagnosticCategory::Source,
            "WARN",
            message,
            SourceSpan {
                file: "internal/common/dto/goal.go".to_string(),
                start_line: 32,
                end_line: 32,
            },
        )
    }

    /// A WritePlan whose every file has the given action (a one-file plan is enough to exercise the
    /// partition + exit policy).
    fn plan_with(action: WriteAction) -> WritePlan {
        WritePlan {
            files: vec![PlannedFile {
                path: "openapi.yaml".to_string(),
                action,
                new_bytes: b"x".to_vec(),
                new_hash: "h".to_string(),
                source: "generated".to_string(),
            }],
        }
    }

    /// A fully clean plan (every output Unchanged).
    fn clean_plan() -> WritePlan {
        plan_with(WriteAction::Unchanged)
    }

    /// Exit-policy (Pitfall 1): a report whose ONLY findings are analysis WARNs is NOT actionable —
    /// the informational WARNs must NOT make doctor red (exit 0).
    #[test]
    fn informational_diagnostics_alone_are_not_actionable() {
        let diags = vec![
            warn_diag("free-form map field: GoalResponse.Metadata (map[string]any) ..."),
            warn_diag("float64 -> float32 narrowing: field CreateGoalInput.TargetValue ..."),
            warn_diag("untyped query param 'cursor' on GET /goal/list ..."),
        ];
        let report = DoctorReport::assemble(
            true, // initialized
            true, // source toolchain present
            "go", // detected source language
            true, // pipeline ran
            Some(diags),
            Some(&clean_plan()),
        );
        assert!(
            !report.has_actionable_problem(),
            "informational WARNs must NOT be actionable (Pitfall 1)"
        );
        assert!(report.healthy, "healthy must be the inverse of actionable");
        assert_eq!(report.summary.actionable_problems, 0);
        assert_eq!(report.summary.informational_diagnostics, 3);
    }

    /// Exit-policy: `.gnr8/` missing (initialized=false) is actionable.
    #[test]
    fn missing_init_is_actionable() {
        let report = DoctorReport::assemble(false, true, "go", false, None, None);
        assert!(report.has_actionable_problem());
        assert!(!report.healthy);
    }

    /// Exit-policy: a missing source toolchain is actionable (and reported, not a crash).
    #[test]
    fn missing_source_toolchain_is_actionable() {
        let report = DoctorReport::assemble(true, false, "go", true, None, Some(&clean_plan()));
        assert!(report.has_actionable_problem());
        assert!(!report.lifecycle.source_toolchain);
    }

    /// The probed toolchain follows the detected source language: a Python source reports
    /// `language == "python"` (and a TypeScript source `"typescript"`), so the report states WHICH
    /// toolchain was probed (RESEARCH A6) rather than hardcoding Go. `assemble` is pure, so the language
    /// label is threaded in by the impure `run_doctor` shell from the single `source_toolchain` decision.
    #[test]
    fn doctor_reports_the_detected_source_language() {
        let py = DoctorReport::assemble(true, true, "python", true, None, Some(&clean_plan()));
        assert_eq!(py.lifecycle.language, "python");
        assert!(py.lifecycle.source_toolchain);
        let text = py.render_human();
        assert!(
            text.contains("python"),
            "the human report must name the probed language:\n{text}"
        );

        let ts = DoctorReport::assemble(true, true, "typescript", true, None, Some(&clean_plan()));
        assert_eq!(ts.lifecycle.language, "typescript");

        // An undetectable source surfaces as a finding (toolchain false + language "unknown"), not a crash.
        let unknown =
            DoctorReport::assemble(true, false, "unknown", true, None, Some(&clean_plan()));
        assert_eq!(unknown.lifecycle.language, "unknown");
        assert!(unknown.has_actionable_problem());
    }

    /// Exit-policy: a pipeline that failed to run is actionable, and drift renders UNKNOWN (never a
    /// false-healthy "up to date").
    #[test]
    fn pipeline_failure_is_actionable_and_drift_unknown() {
        let report = DoctorReport::assemble(true, true, "go", false, None, None);
        assert!(report.has_actionable_problem());
        assert!(!report.lifecycle.pipeline_runs);
        let text = report.render_human();
        assert!(
            !text.contains("all outputs up to date"),
            "a failed pipeline must NOT render as clean:\n{text}"
        );
        assert!(
            text.contains("drift: UNKNOWN"),
            "a failed pipeline must render a distinct drift-unknown finding:\n{text}"
        );
    }

    /// Exit-policy: any stale (`Write`) output is actionable; all-Unchanged is clean.
    #[test]
    fn stale_output_is_actionable_clean_is_not() {
        let stale = DoctorReport::assemble(
            true,
            true,
            "go",
            true,
            None,
            Some(&plan_with(WriteAction::Write)),
        );
        assert!(
            stale.has_actionable_problem(),
            "a stale Write output is actionable"
        );
        assert_eq!(stale.outputs.stale.len(), 1);

        let clean = DoctorReport::assemble(true, true, "go", true, None, Some(&clean_plan()));
        assert!(
            !clean.has_actionable_problem(),
            "an all-Unchanged plan with no other problems is healthy"
        );
        assert_eq!(clean.outputs.unchanged.len(), 1);
    }

    /// Exit-policy: a drifted (`UserEdited`) output is actionable.
    #[test]
    fn drifted_output_is_actionable() {
        let report = DoctorReport::assemble(
            true,
            true,
            "go",
            true,
            None,
            Some(&plan_with(WriteAction::UserEdited)),
        );
        assert!(report.has_actionable_problem());
        assert_eq!(report.outputs.drifted.len(), 1);
    }

    /// `--json` field-set stability (mirrors watch.rs `latency_report_json_field_set`): the serialized
    /// report exposes a stable top-level key set including `healthy`, and `healthy == !actionable`.
    #[test]
    fn doctor_json_field_set() {
        let report = DoctorReport::assemble(
            true,
            true,
            "go",
            true,
            Some(vec![warn_diag("free-form map ... map[string]any ...")]),
            Some(&clean_plan()),
        );
        let value: serde_json::Value = serde_json::to_value(&report).unwrap();
        let obj = value
            .as_object()
            .expect("doctor report serializes to a JSON object");

        let keys: HashSet<&str> = obj.keys().map(String::as_str).collect();
        let expected: HashSet<&str> = [
            "healthy",
            "lifecycle",
            "outputs",
            "diagnostics",
            "sdk_readiness",
            "summary",
            "runtime",
            "timings_ms",
        ]
        .into_iter()
        .collect();
        assert_eq!(keys, expected, "doctor --json field set drifted");

        assert_eq!(
            obj["healthy"],
            serde_json::json!(!report.has_actionable_problem())
        );
        assert_eq!(obj["healthy"], serde_json::json!(true));

        // The lifecycle sub-object exposes the documented facts. The old Go-only toolchain field was
        // generalized to `source_toolchain` and a `language` field added (06-01 / XLANG-04) so the
        // report states WHICH source-language toolchain it probed — this pin locks the new contract.
        let lifecycle = obj["lifecycle"].as_object().expect("lifecycle object");
        let lkeys: HashSet<&str> = lifecycle.keys().map(String::as_str).collect();
        let lexpected: HashSet<&str> = [
            "initialized",
            "source_toolchain",
            "language",
            "pipeline_runs",
        ]
        .into_iter()
        .collect();
        assert_eq!(lkeys, lexpected, "lifecycle --json field set drifted");
        assert_eq!(lifecycle["language"], serde_json::json!("go"));
    }

    #[test]
    fn sdk_readiness_failures_are_actionable() {
        let report = DoctorReport::assemble(true, true, "go", true, None, Some(&clean_plan()))
            .with_sdk_readiness(vec![super::SdkReadiness::not_ready(
                "go",
                "generated/go",
                "go test ./...; go vet ./...",
                "generated package does not compile",
            )]);

        assert!(report.has_actionable_problem());
        assert!(!report.healthy);
        assert_eq!(report.summary.actionable_problems, 1);
        let text = report.render_human();
        assert!(
            text.contains("SDK READINESS"),
            "human report must render SDK readiness:\n{text}"
        );
    }

    /// `render_human` emits the three group headers and a trailing verdict line.
    #[test]
    fn render_human_has_group_headers_and_verdict() {
        let healthy = DoctorReport::assemble(
            true,
            true,
            "go",
            true,
            Some(vec![warn_diag("free-form map ... map[string]any ...")]),
            Some(&clean_plan()),
        );
        let text = healthy.render_human();
        assert!(
            text.contains("LIFECYCLE"),
            "missing LIFECYCLE header:\n{text}"
        );
        assert!(text.contains("OUTPUTS"), "missing OUTPUTS header:\n{text}");
        assert!(
            text.contains("DIAGNOSTICS"),
            "missing DIAGNOSTICS header:\n{text}"
        );
        assert!(
            text.contains("informational"),
            "diagnostics group must be labeled informational:\n{text}"
        );
        assert!(text.contains("healthy"), "missing healthy verdict:\n{text}");

        let unhealthy = DoctorReport::assemble(false, false, "unknown", false, None, None);
        let text = unhealthy.render_human();
        assert!(
            text.contains("actionable problem"),
            "missing actionable verdict:\n{text}"
        );
    }
}
