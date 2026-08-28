//! Host-side pipeline execution: run a [`StagePlan`] in order, natively where possible.
//!
//! The host owns the pipeline. It executes every built-in declaration itself — it already links the
//! extractors, the `OpenAPI` lowering and the SDK emitters — and calls back into the project's worker
//! only for the stages the user wrote. That callback is expressed as [`StageRunner`], so the
//! ordering logic here is tested without a process, and the real implementation
//! ([`crate::worker::WorkerSession`]) is only responsible for the wire.
//!
//! Stage order is composition order. A pipeline with no custom stages never sends a work frame.

use gnr8::sdk::stage::PlanStage;

use crate::graph::{ApiGraph, Diagnostic};
use crate::sdk::{
    builtins, validate_artifact_paths, Artifact, Artifacts, BuiltinTarget, Cx, ReadinessTarget,
    StagePlan,
};
use crate::CoreError;

/// The worker-side half of a pipeline run: whatever executes the user's own stages.
pub trait StageRunner {
    /// Run the custom source at `index`.
    ///
    /// # Errors
    ///
    /// Returns the worker's typed failure.
    fn load_source(&mut self, index: usize) -> Result<ApiGraph, CoreError>;

    /// Run the custom transforms at `indices`, in order, over `graph`.
    ///
    /// # Errors
    ///
    /// Returns the worker's typed failure.
    fn apply_transforms(
        &mut self,
        indices: &[usize],
        graph: ApiGraph,
    ) -> Result<ApiGraph, CoreError>;

    /// Hand over the frozen graph every target runs against, before the first target run.
    ///
    /// Taken by unique reference because a runner that ships it across a process boundary describes
    /// it against what the worker already holds, and lifting its two large vectors out of the way to
    /// do that is cheaper than copying them. The graph is left exactly as it was found.
    ///
    /// # Errors
    ///
    /// Returns the worker's typed failure.
    fn freeze_graph(&mut self, graph: &mut ApiGraph) -> Result<(), CoreError>;

    /// Run the custom targets at `indices`, in order, given the artifacts produced so far.
    ///
    /// # Errors
    ///
    /// Returns the worker's typed failure.
    fn generate_targets(
        &mut self,
        indices: &[usize],
        artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError>;

    /// Run the custom post-processors at `indices`, in order, over `artifacts`.
    ///
    /// # Errors
    ///
    /// Returns the worker's typed failure.
    fn run_posts(
        &mut self,
        indices: &[usize],
        artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError>;
}

/// One contiguous span of a plan's stages that runs in one place.
///
/// The host runs built-ins itself and asks the worker for the user's own stages, so a plan reads as
/// alternating spans. Grouping them is what makes the graph — or the whole artifact set — cross the
/// process boundary once per SPAN rather than once per stage.
#[derive(Debug)]
enum StageSpan<'a, B> {
    /// One built-in stage, with its position in the plan's stage vector.
    Builtin(usize, &'a B),
    /// A run of consecutive custom stages, by their position in the pipeline's custom vector.
    Custom(Vec<usize>),
}

/// Split `stages` into the spans [`StageSpan`] describes, preserving composition order.
fn stage_spans<B>(stages: &[PlanStage<B>]) -> Vec<StageSpan<'_, B>> {
    let mut spans: Vec<StageSpan<'_, B>> = Vec::new();
    for (position, stage) in stages.iter().enumerate() {
        match stage {
            PlanStage::Builtin(spec) => spans.push(StageSpan::Builtin(position, spec)),
            PlanStage::Custom { index, .. } => match spans.last_mut() {
                Some(StageSpan::Custom(indices)) => indices.push(*index),
                _ => spans.push(StageSpan::Custom(vec![*index])),
            },
        }
    }
    spans
}

/// A [`StageRunner`] for a plan that declares no custom stages.
///
/// Not a fallback: a plan either has custom stages, in which case a real worker session serves them,
/// or it has none, in which case any call here is a plan/host disagreement and says so.
pub struct NoCustomStages;

impl StageRunner for NoCustomStages {
    fn load_source(&mut self, index: usize) -> Result<ApiGraph, CoreError> {
        Err(no_custom_stage("source", index))
    }

    fn apply_transforms(
        &mut self,
        indices: &[usize],
        _graph: ApiGraph,
    ) -> Result<ApiGraph, CoreError> {
        Err(no_custom_span("transform", indices))
    }

    fn freeze_graph(&mut self, _graph: &mut ApiGraph) -> Result<(), CoreError> {
        Err(no_custom_span("target", &[]))
    }

    fn generate_targets(
        &mut self,
        indices: &[usize],
        _artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError> {
        Err(no_custom_span("target", indices))
    }

    fn run_posts(
        &mut self,
        indices: &[usize],
        _artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError> {
        Err(no_custom_span("post-process", indices))
    }
}

fn no_custom_stage(kind: &str, index: usize) -> CoreError {
    CoreError::Protocol {
        message: format!(
            "the plan declares no custom {kind} at position {index}, but the host tried to run one"
        ),
    }
}

fn no_custom_span(kind: &str, indices: &[usize]) -> CoreError {
    no_custom_stage(kind, indices.first().copied().unwrap_or_default())
}

/// Everything one pipeline run produced.
#[derive(Debug, Clone)]
pub struct PipelineOutcome {
    /// The generated files, sorted by path.
    pub artifacts: Vec<Artifact>,
    /// Diagnostics the graph carried after transforms.
    pub diagnostics: Vec<Diagnostic>,
    /// Project-relative output anchors declared by every target.
    pub output_anchors: Vec<String>,
    /// Readiness checks declared by every target.
    pub readiness_targets: Vec<ReadinessTarget>,
    /// How many distinct source files contributed a fact to the graph.
    pub source_files: usize,
}

/// The loop-safety anchors a plan's targets declare, plus gnr8's own workspace directory.
#[must_use]
pub fn output_anchors(plan: &StagePlan) -> Vec<String> {
    plan.targets
        .iter()
        .flat_map(|stage| match stage {
            PlanStage::Builtin(spec) => builtins::target_output_anchors(spec),
            PlanStage::Custom { output_anchors, .. } => output_anchors.clone(),
        })
        .collect()
}

/// The readiness checks a plan's targets declare.
#[must_use]
pub fn readiness_targets(plan: &StagePlan) -> Vec<ReadinessTarget> {
    plan.targets
        .iter()
        .flat_map(|stage| match stage {
            PlanStage::Builtin(spec) => builtins::target_readiness_targets(spec),
            PlanStage::Custom {
                readiness_targets, ..
            } => readiness_targets.clone(),
        })
        .collect()
}

/// Project-relative input roots the plan's built-in source declares.
///
/// `gnr8 doctor` probes the source language from these. A custom source declares none — its inputs
/// are its own business — so the answer is empty rather than guessed.
#[must_use]
pub fn source_input_roots(plan: &StagePlan, cx: &Cx) -> Vec<String> {
    plan.sources
        .iter()
        .filter_map(|stage| match stage {
            PlanStage::Builtin(spec) => builtins::source_input_roots(spec, cx),
            PlanStage::Custom { .. } => None,
        })
        .flatten()
        .map(|root| {
            root.strip_prefix(&cx.project_root)
                .unwrap_or(&root)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// Run the plan's source and every transform, and return the graph the targets will see.
///
/// This is the front half of [`run`] and the whole of `gnr8 inspect`.
///
/// # Errors
///
/// Returns [`CoreError::Config`] unless the plan declares exactly one source, or propagates a
/// stage's own typed failure.
pub fn build_ir(
    plan: &StagePlan,
    cx: &Cx,
    runner: &mut dyn StageRunner,
) -> Result<ApiGraph, CoreError> {
    let source = match plan.sources.as_slice() {
        [single] => single,
        [] => {
            return Err(CoreError::Config {
                message: "pipeline has no source — add exactly one `.source(...)` (e.g. \
                          GoGin::new().inputs([\".\"]))"
                    .to_string(),
            });
        }
        many => {
            return Err(CoreError::Config {
                message: format!(
                    "pipeline has {} sources, but merging multiple sources is not yet supported \
                     — configure exactly one `.source(...)`",
                    many.len()
                ),
            });
        }
    };

    let mut ir = match source {
        PlanStage::Builtin(spec) => builtins::load_source(spec, cx)?,
        PlanStage::Custom { index, .. } => runner.load_source(*index)?,
    };

    // Loop safety: drop any operation/schema/diagnostic whose source lives under one of THIS
    // pipeline's own target outputs — or under the `.gnr8/` workspace dir — so a target never
    // re-ingests gnr8's own previously-generated output sitting in the analyzed tree.
    let mut anchors = output_anchors(plan);
    anchors.push(crate::lifecycle::WORKSPACE_DIR.to_string());
    let anchor_refs: Vec<&str> = anchors.iter().map(String::as_str).collect();
    crate::lifecycle::exclude_output_anchors(&mut ir, &anchor_refs);

    for span in stage_spans(&plan.transforms) {
        match span {
            StageSpan::Builtin(_, spec) => builtins::apply_transform(spec, &mut ir, cx)?,
            StageSpan::Custom(indices) => ir = runner.apply_transforms(&indices, ir)?,
        }
    }
    Ok(ir)
}

/// Run the whole plan: source → transforms → freeze → each target → each post-processor.
///
/// # Errors
///
/// Propagates any stage's typed failure, or [`CoreError::ArtifactOwnership`] when the finished
/// artifact set contains a path this host cannot portably write.
pub fn run(
    plan: &StagePlan,
    cx: &Cx,
    runner: &mut dyn StageRunner,
) -> Result<PipelineOutcome, CoreError> {
    let ir = build_ir(plan, cx, runner)?;
    let diagnostics: Vec<Diagnostic> = ir.diagnostics.clone();
    let source_files = distinct_source_files(&ir);

    let mut artifacts = Artifacts::new();
    if !plan.targets.is_empty() {
        // Every target, including a user-defined one, receives the same canonical directional
        // graph. `build_ir` and `inspect` intentionally retain the unsplit source facts; the
        // projection belongs at the artifact boundary.
        let mut generation_ir = crate::graph::projection::into_generation(ir)?;
        let spans = stage_spans(&plan.targets);
        // The frozen graph is the same for every target, so it crosses to the worker once rather
        // than riding along with each run.
        if spans
            .iter()
            .any(|span| matches!(span, StageSpan::Custom(_)))
        {
            runner.freeze_graph(&mut generation_ir)?;
        }
        for span in spans {
            match span {
                StageSpan::Builtin(position, spec) => {
                    artifacts.begin_stage(builtin_target_producer(position, spec));
                    builtins::generate_target(spec, &generation_ir, &mut artifacts, cx)?;
                }
                StageSpan::Custom(indices) => {
                    let sent = artifacts.into_files();
                    let paths = artifact_paths(&sent);
                    let produced = runner.generate_targets(&indices, sent)?;
                    require_no_dropped_artifacts("target", &indices, &paths, &produced)?;
                    artifacts = Artifacts::from_files(produced);
                }
            }
        }
    }

    for span in stage_spans(&plan.posts) {
        match span {
            StageSpan::Builtin(position, spec) => {
                artifacts.begin_stage(format!("post[{position}]:{}", spec.label()));
                builtins::run_post(spec, &mut artifacts, cx)?;
            }
            StageSpan::Custom(indices) => {
                let sent = artifacts.into_files();
                let paths = artifact_paths(&sent);
                let produced = runner.run_posts(&indices, sent)?;
                require_no_dropped_artifacts("post-process", &indices, &paths, &produced)?;
                artifacts = Artifacts::from_files(produced);
            }
        }
    }

    let artifacts = artifacts.into_files();
    validate_artifact_paths(&artifacts)?;
    Ok(PipelineOutcome {
        artifacts,
        diagnostics,
        output_anchors: output_anchors(plan),
        readiness_targets: readiness_targets(plan),
        source_files,
    })
}

/// A stage may create, overlay or rewrite an artifact. It may not make one disappear.
///
/// Inside either process that is guaranteed by construction — [`Artifacts`] has no removal API, and
/// a run of stages shares one accumulator. Across the wire it is not: a reply is just a list, and a
/// worker that returned the wrong one would have the host treat another target's output as stale and
/// delete it from disk. So the host checks what came back against what it sent.
/// The paths of an artifact set, kept while the set itself is handed to the worker.
///
/// Only the paths are needed to police the reply, so the set is MOVED into the request rather than
/// cloned: on a large SDK a clone here duplicated every generated file in memory for a membership
/// check.
fn artifact_paths(artifacts: &[Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .map(|artifact| artifact.path.clone())
        .collect()
}

fn require_no_dropped_artifacts(
    kind: &str,
    indices: &[usize],
    sent: &[String],
    produced: &[Artifact],
) -> Result<(), CoreError> {
    let kept: std::collections::BTreeSet<&str> = produced
        .iter()
        .map(|artifact| artifact.path.as_str())
        .collect();
    if let Some(dropped) = sent.iter().find(|path| !kept.contains(path.as_str())) {
        let index = indices.first().copied().unwrap_or_default();
        return Err(CoreError::Protocol {
            message: format!(
                "custom {kind} #{index} returned an artifact set that no longer contains \
                 {dropped:?}, which an earlier stage produced; a stage may create, overlay or \
                 rewrite an artifact but never drop one"
            ),
        });
    }
    Ok(())
}

fn builtin_target_producer(index: usize, spec: &BuiltinTarget) -> String {
    format!("target[{index}]:{}", spec.label())
}

/// How many distinct source files contributed a fact to the graph.
///
/// This is what `gnr8 generate -v` reports as "parsed/input files": the files the extraction
/// actually drew provenance from, rather than a count of everything under an input directory.
fn distinct_source_files(ir: &ApiGraph) -> usize {
    let mut files: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for operation in &ir.operations {
        files.insert(operation.provenance.file.as_str());
        for param in &operation.params {
            files.insert(param.provenance.file.as_str());
        }
    }
    for schema in &ir.schemas {
        files.insert(schema.provenance.file.as_str());
    }
    for diagnostic in &ir.diagnostics {
        files.insert(diagnostic.file.as_str());
    }
    files.remove("");
    files.len()
}

/// A [`StageRunner`] that executes a composed [`Pipeline`]'s custom stages in this process.
///
/// The CLI never uses this: it always talks to a project's worker, because that is where a user's
/// Rust belongs. It exists for the two callers that already hold the `Pipeline` value — gnr8's own
/// contract tests, and anything embedding the engine as a library — so they exercise the exact same
/// [`run`] with the exact same ordering rules rather than a parallel implementation.
pub struct InProcessRunner<'a> {
    pipeline: &'a crate::sdk::Pipeline,
    cx: &'a Cx,
    frozen: Option<ApiGraph>,
}

impl<'a> InProcessRunner<'a> {
    /// Run `pipeline`'s custom stages in this process, resolving relative paths against `cx`.
    #[must_use]
    pub const fn new(pipeline: &'a crate::sdk::Pipeline, cx: &'a Cx) -> Self {
        Self {
            pipeline,
            cx,
            frozen: None,
        }
    }
}

impl StageRunner for InProcessRunner<'_> {
    fn load_source(&mut self, index: usize) -> Result<ApiGraph, CoreError> {
        let source = self
            .pipeline
            .custom_source(index)
            .ok_or_else(|| no_custom_stage("source", index))?;
        Ok(source.load(self.cx)?)
    }

    fn apply_transforms(
        &mut self,
        indices: &[usize],
        mut graph: ApiGraph,
    ) -> Result<ApiGraph, CoreError> {
        for &index in indices {
            let transform = self
                .pipeline
                .custom_transform(index)
                .ok_or_else(|| no_custom_stage("transform", index))?;
            transform.apply(&mut graph, self.cx)?;
        }
        Ok(graph)
    }

    fn freeze_graph(&mut self, graph: &mut ApiGraph) -> Result<(), CoreError> {
        self.frozen = Some(graph.clone());
        Ok(())
    }

    fn generate_targets(
        &mut self,
        indices: &[usize],
        artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError> {
        let graph = self.frozen.as_ref().ok_or_else(|| CoreError::Protocol {
            message: "a custom target ran before the frozen graph was handed over".to_string(),
        })?;
        let mut out = Artifacts::from_files(artifacts);
        for &index in indices {
            let target = self
                .pipeline
                .custom_target(index)
                .ok_or_else(|| no_custom_stage("target", index))?;
            out.begin_stage(format!("target[{index}]:{}", target.producer()));
            target.generate(graph, &mut out, self.cx)?;
        }
        Ok(out.into_files())
    }

    fn run_posts(
        &mut self,
        indices: &[usize],
        artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError> {
        let mut out = Artifacts::from_files(artifacts);
        for &index in indices {
            let post = self
                .pipeline
                .custom_post(index)
                .ok_or_else(|| no_custom_stage("post-process", index))?;
            out.begin_stage(format!("post[{index}]:{}", post.producer()));
            post.run(&mut out, self.cx)?;
        }
        Ok(out.into_files())
    }
}

/// Run `pipeline` end to end in this process.
///
/// # Errors
///
/// Propagates any stage's typed failure.
pub fn run_in_process(
    pipeline: &crate::sdk::Pipeline,
    cx: &Cx,
) -> Result<PipelineOutcome, CoreError> {
    let plan = pipeline.plan();
    let mut runner = InProcessRunner::new(pipeline, cx);
    run(&plan, cx, &mut runner)
}

/// Build `pipeline`'s post-transform graph in this process.
///
/// # Errors
///
/// Propagates any stage's typed failure.
pub fn build_ir_in_process(
    pipeline: &crate::sdk::Pipeline,
    cx: &Cx,
) -> Result<ApiGraph, CoreError> {
    let plan = pipeline.plan();
    let mut runner = InProcessRunner::new(pipeline, cx);
    build_ir(&plan, cx, &mut runner)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{build_ir, run, NoCustomStages, StageRunner};
    use crate::graph::ApiGraph;
    use crate::sdk::{builtins as decl, Artifact, Custom, Cx, Pipeline, Target, Transform};
    use crate::CoreError;
    use gnr8::sdk::{Artifacts, StagePlan};

    /// A runner that records the calls the host made, and answers them deterministically.
    #[derive(Default)]
    struct RecordingRunner {
        calls: Vec<String>,
        frozen: Option<ApiGraph>,
    }

    impl StageRunner for RecordingRunner {
        fn load_source(&mut self, index: usize) -> Result<ApiGraph, CoreError> {
            self.calls.push(format!("source[{index}]"));
            Ok(ApiGraph {
                title: "from-worker".to_string(),
                ..ApiGraph::default()
            })
        }

        fn apply_transforms(
            &mut self,
            indices: &[usize],
            mut graph: ApiGraph,
        ) -> Result<ApiGraph, CoreError> {
            self.calls.push(format!("transforms{indices:?}"));
            for index in indices {
                graph.title = format!("{}+t{index}", graph.title);
            }
            Ok(graph)
        }

        fn freeze_graph(&mut self, graph: &mut ApiGraph) -> Result<(), CoreError> {
            self.calls.push("freeze".to_string());
            self.frozen = Some(graph.clone());
            Ok(())
        }

        fn generate_targets(
            &mut self,
            indices: &[usize],
            mut artifacts: Vec<Artifact>,
        ) -> Result<Vec<Artifact>, CoreError> {
            self.calls.push(format!("targets{indices:?}"));
            let title = self
                .frozen
                .as_ref()
                .map_or("", |graph| graph.title.as_str());
            for index in indices {
                artifacts.push(Artifact::new(
                    format!("generated/custom-{index}.md"),
                    format!("# {title}\n"),
                ));
            }
            artifacts.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(artifacts)
        }

        fn run_posts(
            &mut self,
            indices: &[usize],
            mut artifacts: Vec<Artifact>,
        ) -> Result<Vec<Artifact>, CoreError> {
            self.calls.push(format!("posts{indices:?}"));
            for _ in indices {
                for artifact in &mut artifacts {
                    artifact.text = format!("//post\n{}", artifact.text);
                }
            }
            Ok(artifacts)
        }
    }

    struct CustomSource;
    impl crate::sdk::Source for CustomSource {
        fn load(&self, _cx: &Cx) -> Result<ApiGraph, gnr8::Error> {
            Ok(ApiGraph::default())
        }
    }

    struct CustomTransform;
    impl Transform for CustomTransform {
        fn apply(&self, _ir: &mut ApiGraph, _cx: &Cx) -> Result<(), gnr8::Error> {
            Ok(())
        }
    }

    struct CustomTarget;
    impl Target for CustomTarget {
        fn generate(
            &self,
            _ir: &ApiGraph,
            _out: &mut Artifacts,
            _cx: &Cx,
        ) -> Result<(), gnr8::Error> {
            Ok(())
        }

        fn output_anchors(&self) -> Vec<String> {
            vec!["generated/custom-0.md".to_string()]
        }
    }

    fn cx() -> Cx {
        Cx::new(std::env::temp_dir())
    }

    #[test]
    fn a_plan_with_no_source_is_a_config_error() {
        let err = build_ir(&StagePlan::default(), &cx(), &mut NoCustomStages).unwrap_err();
        assert!(matches!(err, CoreError::Config { .. }), "{err:?}");
    }

    #[test]
    fn two_sources_are_rejected_rather_than_silently_merged() {
        let plan = Pipeline::new()
            .source(Custom(CustomSource))
            .source(Custom(CustomSource))
            .plan();
        let err = build_ir(&plan, &cx(), &mut RecordingRunner::default()).unwrap_err();
        assert!(err.to_string().contains("2 sources"), "{err}");
    }

    #[test]
    fn custom_stages_run_in_composition_order() {
        let plan = Pipeline::new()
            .source(Custom(CustomSource))
            .transform(Custom(CustomTransform))
            .transform(decl::SetTitle::new("Renamed"))
            .transform(Custom(CustomTransform))
            .target(Custom(CustomTarget))
            .plan();
        let mut runner = RecordingRunner::default();
        let outcome = run(&plan, &cx(), &mut runner).unwrap();

        // The two custom transforms are separated by a built-in, so they are two runs; a run of
        // adjacent customs would have been one request.
        assert_eq!(
            runner.calls,
            vec![
                "source[0]",
                "transforms[0]",
                "transforms[2]",
                "freeze",
                "targets[0]"
            ]
        );
        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(outcome.artifacts[0].path, "generated/custom-0.md");
        // The built-in transform ran host-side, between the two worker calls.
        assert_eq!(outcome.artifacts[0].text, "# Renamed+t2\n");
        assert_eq!(
            outcome.output_anchors,
            vec!["generated/custom-0.md".to_string()]
        );
    }

    #[test]
    fn a_plan_without_custom_stages_never_calls_the_runner() {
        let plan = Pipeline::new()
            .source(Custom(CustomSource))
            .transform(decl::SetTitle::new("Only built-ins"))
            .plan();
        // The source is the only custom stage; nothing else may reach the runner.
        let mut runner = RecordingRunner::default();
        run(&plan, &cx(), &mut runner).unwrap();
        assert_eq!(runner.calls, vec!["source[0]"]);
    }

    #[test]
    fn an_unportable_artifact_path_from_a_worker_is_rejected_before_writing() {
        struct EscapingRunner;
        impl StageRunner for EscapingRunner {
            fn load_source(&mut self, _index: usize) -> Result<ApiGraph, CoreError> {
                Ok(ApiGraph::default())
            }
            fn apply_transforms(
                &mut self,
                _indices: &[usize],
                graph: ApiGraph,
            ) -> Result<ApiGraph, CoreError> {
                Ok(graph)
            }
            fn freeze_graph(&mut self, _graph: &mut ApiGraph) -> Result<(), CoreError> {
                Ok(())
            }
            fn generate_targets(
                &mut self,
                _indices: &[usize],
                _artifacts: Vec<Artifact>,
            ) -> Result<Vec<Artifact>, CoreError> {
                Ok(vec![Artifact::new("../escape.txt", "x")])
            }
            fn run_posts(
                &mut self,
                _indices: &[usize],
                artifacts: Vec<Artifact>,
            ) -> Result<Vec<Artifact>, CoreError> {
                Ok(artifacts)
            }
        }

        let plan = Pipeline::new()
            .source(Custom(CustomSource))
            .target(Custom(CustomTarget))
            .plan();
        let err = run(&plan, &cx(), &mut EscapingRunner).unwrap_err();
        assert!(
            matches!(err, CoreError::ArtifactOwnership { ref code, .. } if code == "artifact.path_invalid"),
            "{err:?}"
        );
    }

    #[test]
    fn a_worker_that_drops_an_earlier_stages_artifact_is_rejected() {
        struct DroppingRunner;
        impl StageRunner for DroppingRunner {
            fn load_source(&mut self, _index: usize) -> Result<ApiGraph, CoreError> {
                Ok(ApiGraph::default())
            }
            fn apply_transforms(
                &mut self,
                _indices: &[usize],
                graph: ApiGraph,
            ) -> Result<ApiGraph, CoreError> {
                Ok(graph)
            }
            fn freeze_graph(&mut self, _graph: &mut ApiGraph) -> Result<(), CoreError> {
                Ok(())
            }
            fn generate_targets(
                &mut self,
                _indices: &[usize],
                _artifacts: Vec<Artifact>,
            ) -> Result<Vec<Artifact>, CoreError> {
                // Answers with only its own file, discarding whatever the OpenAPI target produced.
                Ok(vec![Artifact::new("generated/only-mine.md", "x")])
            }
            fn run_posts(
                &mut self,
                _indices: &[usize],
                artifacts: Vec<Artifact>,
            ) -> Result<Vec<Artifact>, CoreError> {
                Ok(artifacts)
            }
        }

        let plan = Pipeline::new()
            .source(Custom(CustomSource))
            .target(decl::OpenApi31::new().to("generated/openapi.yaml"))
            .target(Custom(CustomTarget))
            .plan();
        let err = run(&plan, &cx(), &mut DroppingRunner).unwrap_err();
        assert!(
            err.to_string().contains("never drop one"),
            "a dropped artifact would be deleted from disk as stale: {err}"
        );
    }

    #[test]
    fn a_case_fold_alias_between_two_targets_is_a_collision() {
        struct AliasRunner(usize);
        impl StageRunner for AliasRunner {
            fn load_source(&mut self, _index: usize) -> Result<ApiGraph, CoreError> {
                Ok(ApiGraph::default())
            }
            fn apply_transforms(
                &mut self,
                _indices: &[usize],
                graph: ApiGraph,
            ) -> Result<ApiGraph, CoreError> {
                Ok(graph)
            }
            fn freeze_graph(&mut self, _graph: &mut ApiGraph) -> Result<(), CoreError> {
                Ok(())
            }
            fn generate_targets(
                &mut self,
                indices: &[usize],
                mut artifacts: Vec<Artifact>,
            ) -> Result<Vec<Artifact>, CoreError> {
                for _ in indices {
                    self.0 += 1;
                    artifacts.push(Artifact::new(
                        if self.0 == 1 {
                            "out/File.txt"
                        } else {
                            "out/file.txt"
                        },
                        "x",
                    ));
                }
                Ok(artifacts)
            }
            fn run_posts(
                &mut self,
                _indices: &[usize],
                artifacts: Vec<Artifact>,
            ) -> Result<Vec<Artifact>, CoreError> {
                Ok(artifacts)
            }
        }

        let plan = Pipeline::new()
            .source(Custom(CustomSource))
            .target(Custom(CustomTarget))
            .target(Custom(CustomTarget))
            .plan();
        let err = run(&plan, &cx(), &mut AliasRunner(0)).unwrap_err();
        assert!(
            matches!(err, CoreError::ArtifactOwnership { ref code, .. } if code == "artifact.path_collision"),
            "{err:?}"
        );
    }
}
