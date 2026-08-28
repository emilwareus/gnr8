//! The worker runtime: the entry point a project's `.gnr8/` binary calls from `main()`.
//!
//! ```no_run
//! use gnr8::sdk::prelude::*;
//!
//! fn main() -> std::process::ExitCode {
//!     gnr8::worker::run(
//!         Pipeline::new()
//!             .source(GoGin::new().inputs(["."]))
//!             .transform(SetTitle::new("Bookstore API"))
//!             .target(OpenApi31::new().to("generated/openapi.yaml")),
//!     )
//! }
//! ```
//!
//! [`run`] speaks the frame protocol on stdin/stdout: it waits for the host's `Hello`, answers with
//! the [`StagePlan`](crate::sdk::StagePlan), then serves one request per custom stage until the host
//! sends `Shutdown`. Built-in stages never reach this process as work — the host executes them.
//!
//! Exit codes: `0` on a clean shutdown, `1` if a stage failed, `2` if the handshake is missing or
//! mismatched (which is what happens when the binary is run by hand rather than by `gnr8`).
//!
//! [`run`] NEVER panics: every fallible step returns a typed [`crate::Error`] that is reported to the
//! host as a `Failed` frame where possible and to stderr otherwise (RUST-04).

use std::io::{Read, Write};
use std::process::ExitCode;

use crate::protocol::{
    capability_digest, read_frame, sdk_version, write_frame, HostMessage, WorkerMessage,
    PROTOCOL_VERSION,
};
use crate::sdk::{Artifacts, Cx, Pipeline};
use crate::Error;

/// Exit code for a handshake failure — including running the worker binary by hand.
const EXIT_HANDSHAKE: u8 = 2;

/// Serve `pipeline` to the `gnr8` host over stdin/stdout.
///
/// The by-value `Pipeline` is the public contract a `.gnr8/` `main()` calls: it hands the composed
/// pipeline over wholesale.
#[allow(clippy::needless_pass_by_value)]
#[must_use]
pub fn run(pipeline: Pipeline) -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match serve(&pipeline, &mut stdin.lock(), &mut stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Session::Handshake(err)) => {
            eprintln!("gnr8 worker: {err}");
            eprintln!(
                "this binary is the gnr8 generation worker; it is started by the `gnr8` host, which \
                 speaks a framed protocol on its stdin/stdout. Run `gnr8 generate` instead."
            );
            ExitCode::from(EXIT_HANDSHAKE)
        }
        Err(Session::Run(err)) => {
            eprintln!("gnr8 worker: {err}");
            ExitCode::FAILURE
        }
    }
}

/// How a session ended, so [`run`] can pick the right exit code.
pub(crate) enum Session {
    /// The host handshake was absent or mismatched.
    Handshake(Error),
    /// A stage or a frame failed after the session opened.
    Run(Error),
}

/// Drive one session over `input`/`output`.
///
/// Split out from [`run`] so the loop is testable over in-memory pipes.
///
/// # Errors
///
/// Returns the session-ending failure. A stage error is reported to the host as a `Failed` frame
/// first, so the host can attribute it, and then returned here.
pub(crate) fn serve<R: Read, W: Write>(
    pipeline: &Pipeline,
    input: &mut R,
    output: &mut W,
) -> Result<(), Session> {
    let cx = handshake(pipeline, input, output)?;
    // The frozen graph the target phase runs against, once the host has handed it over.
    let mut frozen: Option<crate::graph::ApiGraph> = None;
    loop {
        let request: HostMessage = read_frame(input).map_err(Session::Run)?;
        match request {
            HostMessage::Hello { .. } => {
                let err = Error::protocol("the host sent a second handshake in one session");
                report(output, &err);
                return Err(Session::Run(err));
            }
            HostMessage::Shutdown => {
                write_frame(output, &WorkerMessage::Done).map_err(Session::Run)?;
                return Ok(());
            }
            HostMessage::FreezeGraph { graph } => {
                frozen = Some(graph);
                write_frame(output, &WorkerMessage::Done).map_err(Session::Run)?;
            }
            other => match dispatch(pipeline, &cx, frozen.as_ref(), other) {
                Ok(reply) => write_frame(output, &reply).map_err(Session::Run)?,
                Err(err) => {
                    report(output, &err);
                    return Err(Session::Run(err));
                }
            },
        }
    }
}

/// Read the host's `Hello`, validate it, and answer with the plan.
fn handshake<R: Read, W: Write>(
    pipeline: &Pipeline,
    input: &mut R,
    output: &mut W,
) -> Result<Cx, Session> {
    let hello: HostMessage = read_frame(input).map_err(Session::Handshake)?;
    let HostMessage::Hello {
        protocol,
        host_version,
        capability_digest: host_digest,
        project_root,
    } = hello
    else {
        return Err(Session::Handshake(Error::protocol(
            "the first frame of a session must be the host handshake",
        )));
    };
    let expected_digest = capability_digest(sdk_version());
    if protocol != PROTOCOL_VERSION {
        let err = Error::protocol(format!(
            "host protocol {protocol} does not match worker protocol {PROTOCOL_VERSION}; align the \
             installed gnr8 CLI with .gnr8/Cargo.lock"
        ));
        report(output, &err);
        return Err(Session::Handshake(err));
    }
    if host_version != sdk_version() {
        let err = Error::protocol(format!(
            "host CLI {host_version} does not match the gnr8 SDK {} linked into this worker; \
             install the exact version pinned in .gnr8/Cargo.toml",
            sdk_version()
        ));
        report(output, &err);
        return Err(Session::Handshake(err));
    }
    if host_digest != expected_digest {
        let err = Error::protocol(format!(
            "host capability digest {host_digest} does not match worker digest {expected_digest}; \
             rebuild both sides at one exact version"
        ));
        report(output, &err);
        return Err(Session::Handshake(err));
    }
    write_frame(
        output,
        &WorkerMessage::Ready {
            protocol: PROTOCOL_VERSION,
            sdk_version: sdk_version().to_string(),
            capability_digest: expected_digest,
            plan: pipeline.plan(),
        },
    )
    .map_err(Session::Handshake)?;
    Ok(Cx::new(project_root))
}

/// Run one request's whole run of custom stages and build the reply frame.
///
/// The stages in a run execute in the order the host listed them, against ONE accumulator: an
/// [`Artifacts`] has no removal API, so the "a stage may create, overlay or rewrite an artifact but
/// never drop one" rule holds between the stages of a run by construction, exactly as it does for a
/// run of one.
fn dispatch(
    pipeline: &Pipeline,
    cx: &Cx,
    frozen: Option<&crate::graph::ApiGraph>,
    request: HostMessage,
) -> Result<WorkerMessage, Error> {
    match request {
        HostMessage::LoadSource { index } => {
            let source = pipeline
                .custom_source(index)
                .ok_or_else(|| unknown_stage("source", index))?;
            Ok(WorkerMessage::Graph {
                graph: source.load(cx)?,
            })
        }
        HostMessage::ApplyTransforms { indices, mut graph } => {
            for index in indices {
                let transform = pipeline
                    .custom_transform(index)
                    .ok_or_else(|| unknown_stage("transform", index))?;
                transform.apply(&mut graph, cx)?;
            }
            Ok(WorkerMessage::Graph { graph })
        }
        HostMessage::GenerateTargets { indices, artifacts } => {
            let graph = frozen.ok_or_else(|| {
                Error::protocol(
                    "the host asked for a custom target before handing over the frozen graph",
                )
            })?;
            let received = artifacts.clone();
            let mut out = Artifacts::from_files(artifacts);
            for index in indices {
                let target = pipeline
                    .custom_target(index)
                    .ok_or_else(|| unknown_stage("target", index))?;
                out.begin_stage(format!("target[{index}]:{}", target.producer()));
                target.generate(graph, &mut out, cx)?;
            }
            Ok(WorkerMessage::ArtifactChanges {
                changed: artifact_changes(&received, out.into_files()),
            })
        }
        HostMessage::RunPosts { indices, artifacts } => {
            let received = artifacts.clone();
            let mut out = Artifacts::from_files(artifacts);
            for index in indices {
                let post = pipeline
                    .custom_post(index)
                    .ok_or_else(|| unknown_stage("post-process", index))?;
                out.begin_stage(format!("post[{index}]:{}", post.producer()));
                post.run(&mut out, cx)?;
            }
            Ok(WorkerMessage::ArtifactChanges {
                changed: artifact_changes(&received, out.into_files()),
            })
        }
        HostMessage::Hello { .. } | HostMessage::Shutdown | HostMessage::FreezeGraph { .. } => {
            Err(Error::protocol(
                "handshake, graph handover and shutdown are handled by the session loop",
            ))
        }
    }
}

/// The artifacts in `after` that `before` did not already hold identically.
///
/// Both are sorted by path and `after` is a superset of `before` by path — a stage can create,
/// overlay or rewrite, never drop — so one merge walk answers it.
fn artifact_changes(
    before: &[crate::sdk::Artifact],
    after: Vec<crate::sdk::Artifact>,
) -> Vec<crate::sdk::Artifact> {
    let mut kept = before.iter().peekable();
    let mut changed = Vec::new();
    for artifact in after {
        while kept.peek().is_some_and(|held| held.path < artifact.path) {
            kept.next();
        }
        let held = kept
            .next_if(|held| held.path == artifact.path)
            .is_some_and(|held| *held == artifact);
        if !held {
            changed.push(artifact);
        }
    }
    changed
}

fn unknown_stage(kind: &str, index: usize) -> Error {
    Error::protocol(format!(
        "the host asked for custom {kind} #{index}, but this pipeline has no custom {kind} at that \
         position; the host and worker disagree about the plan"
    ))
}

/// Best-effort `Failed` frame so the host attributes the error to the worker rather than to a
/// closed pipe. A write failure here is already terminal, so it is deliberately dropped.
fn report<W: Write>(output: &mut W, err: &Error) {
    let _ = write_frame(
        output,
        &WorkerMessage::Failed {
            message: err.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{serve, Session};
    use crate::graph::ApiGraph;
    use crate::protocol::{
        capability_digest, read_frame, sdk_version, write_frame, HostMessage, WorkerMessage,
        PROTOCOL_VERSION,
    };
    use crate::sdk::{Artifacts, Custom, Cx, Pipeline, Target, Transform};
    use crate::Error;

    struct SetMarker;
    impl Transform for SetMarker {
        fn apply(&self, ir: &mut ApiGraph, _cx: &Cx) -> Result<(), Error> {
            ir.title = format!("{}::marked", ir.title);
            Ok(())
        }
    }

    struct FailingTransform;
    impl Transform for FailingTransform {
        fn apply(&self, _ir: &mut ApiGraph, _cx: &Cx) -> Result<(), Error> {
            Err(Error::generation("stage exploded"))
        }
    }

    struct MarkdownTarget;
    impl Target for MarkdownTarget {
        fn generate(&self, ir: &ApiGraph, out: &mut Artifacts, _cx: &Cx) -> Result<(), Error> {
            out.create("generated/API.md", format!("# {}\n", ir.title))
        }

        fn output_anchors(&self) -> Vec<String> {
            vec!["generated/API.md".to_string()]
        }
    }

    fn hello() -> HostMessage {
        HostMessage::Hello {
            protocol: PROTOCOL_VERSION,
            host_version: sdk_version().to_string(),
            capability_digest: capability_digest(sdk_version()),
            project_root: "/repo".to_string(),
        }
    }

    fn drive(pipeline: &Pipeline, requests: &[HostMessage]) -> (Result<(), Session>, Vec<u8>) {
        let mut input = Vec::new();
        for request in requests {
            write_frame(&mut input, request).unwrap();
        }
        let mut output = Vec::new();
        let result = serve(pipeline, &mut input.as_slice(), &mut output);
        (result, output)
    }

    fn replies(output: &[u8]) -> Vec<WorkerMessage> {
        let mut cursor = output;
        let mut out = Vec::new();
        while !cursor.is_empty() {
            out.push(read_frame::<_, WorkerMessage>(&mut cursor).unwrap());
        }
        out
    }

    #[test]
    fn a_session_answers_the_plan_then_serves_custom_stages() {
        let pipeline = Pipeline::new()
            .source(crate::sdk::builtins::GoGin::new().inputs(["."]))
            .transform(Custom(SetMarker))
            .target(Custom(MarkdownTarget));
        let graph = ApiGraph {
            title: "Base".to_string(),
            ..ApiGraph::default()
        };
        let (result, output) = drive(
            &pipeline,
            &[
                hello(),
                HostMessage::ApplyTransforms {
                    indices: vec![0],
                    graph: graph.clone(),
                },
                HostMessage::FreezeGraph {
                    graph: ApiGraph {
                        title: "Base::marked".to_string(),
                        ..ApiGraph::default()
                    },
                },
                HostMessage::GenerateTargets {
                    indices: vec![0],
                    artifacts: Vec::new(),
                },
                HostMessage::Shutdown,
            ],
        );
        assert!(result.is_ok());
        let messages = replies(&output);
        assert!(matches!(messages[0], WorkerMessage::Ready { .. }));
        let WorkerMessage::Graph { graph } = &messages[1] else {
            panic!("expected a graph reply, got {:?}", messages[1]);
        };
        assert_eq!(graph.title, "Base::marked");
        assert!(matches!(messages[2], WorkerMessage::Done));
        let WorkerMessage::ArtifactChanges { changed: artifacts } = &messages[3] else {
            panic!("expected artifact changes, got {:?}", messages[3]);
        };
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "generated/API.md");
        assert_eq!(artifacts[0].text, "# Base::marked\n");
        assert!(artifacts[0].producer.starts_with("target[0]:"));
        assert!(matches!(messages[4], WorkerMessage::Done));
    }

    /// A target request that arrives before the graph handover is a protocol error, not a guess.
    #[test]
    fn a_custom_target_before_the_frozen_graph_is_refused() {
        let pipeline = Pipeline::new().target(Custom(MarkdownTarget));
        let (result, output) = drive(
            &pipeline,
            &[
                hello(),
                HostMessage::GenerateTargets {
                    indices: vec![0],
                    artifacts: Vec::new(),
                },
            ],
        );
        assert!(matches!(result, Err(Session::Run(_))));
        let WorkerMessage::Failed { message } = &replies(&output)[1] else {
            panic!("expected Failed");
        };
        assert!(message.contains("frozen graph"), "{message}");
    }

    #[test]
    fn a_session_serves_a_custom_source_and_post_processor() {
        struct FixedSource;
        impl crate::sdk::Source for FixedSource {
            fn load(&self, cx: &Cx) -> Result<ApiGraph, Error> {
                Ok(ApiGraph {
                    title: cx.project_root.to_string_lossy().into_owned(),
                    ..ApiGraph::default()
                })
            }
        }

        struct Banner;
        impl crate::sdk::PostProcess for Banner {
            fn run(&self, out: &mut Artifacts, _cx: &Cx) -> Result<(), Error> {
                out.rewrite("a.txt", |text| format!("// banner\n{text}"))
            }
        }

        let pipeline = Pipeline::new()
            .source(Custom(FixedSource))
            .post(Custom(Banner));
        let (result, output) = drive(
            &pipeline,
            &[
                hello(),
                HostMessage::LoadSource { index: 0 },
                HostMessage::RunPosts {
                    indices: vec![0],
                    artifacts: vec![crate::sdk::Artifact::new("a.txt", "body\n")],
                },
                HostMessage::Shutdown,
            ],
        );
        assert!(result.is_ok());
        let messages = replies(&output);
        let WorkerMessage::Graph { graph } = &messages[1] else {
            panic!("expected a graph, got {:?}", messages[1]);
        };
        // The worker's `Cx` is the project root the host declared in its handshake.
        assert_eq!(graph.title, "/repo");
        let WorkerMessage::ArtifactChanges { changed: artifacts } = &messages[2] else {
            panic!("expected artifact changes, got {:?}", messages[2]);
        };
        assert_eq!(artifacts.len(), 1, "only the rewritten artifact comes back");
        assert_eq!(artifacts[0].text, "// banner\nbody\n");
        assert!(artifacts[0].producer.starts_with("post[0]:"));
    }

    /// The reply is what the run changed, so an untouched artifact is not shipped back at all.
    #[test]
    fn an_untouched_artifact_is_not_in_the_reply() {
        struct TouchOne;
        impl crate::sdk::PostProcess for TouchOne {
            fn run(&self, out: &mut Artifacts, _cx: &Cx) -> Result<(), Error> {
                out.rewrite("b.txt", |text| format!("{text}!"))
            }
        }
        let pipeline = Pipeline::new().post(Custom(TouchOne));
        let (result, output) = drive(
            &pipeline,
            &[
                hello(),
                HostMessage::RunPosts {
                    indices: vec![0],
                    artifacts: vec![
                        crate::sdk::Artifact::new("a.txt", "untouched"),
                        crate::sdk::Artifact::new("b.txt", "body"),
                    ],
                },
                HostMessage::Shutdown,
            ],
        );
        assert!(result.is_ok());
        let WorkerMessage::ArtifactChanges { changed } = &replies(&output)[1] else {
            panic!("expected artifact changes");
        };
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert_eq!(changed[0].path, "b.txt");
        assert_eq!(changed[0].text, "body!");
    }

    #[test]
    fn a_stage_failure_is_reported_as_a_failed_frame() {
        let pipeline = Pipeline::new().transform(Custom(FailingTransform));
        let (result, output) = drive(
            &pipeline,
            &[
                hello(),
                HostMessage::ApplyTransforms {
                    indices: vec![0],
                    graph: ApiGraph::default(),
                },
            ],
        );
        assert!(matches!(result, Err(Session::Run(_))));
        let messages = replies(&output);
        let WorkerMessage::Failed { message } = &messages[1] else {
            panic!("expected Failed, got {:?}", messages[1]);
        };
        assert!(message.contains("stage exploded"), "{message}");
    }

    #[test]
    fn a_protocol_skew_fails_the_handshake_before_any_stage_runs() {
        let pipeline = Pipeline::new().transform(Custom(FailingTransform));
        let (result, output) = drive(
            &pipeline,
            &[HostMessage::Hello {
                protocol: PROTOCOL_VERSION + 1,
                host_version: sdk_version().to_string(),
                capability_digest: capability_digest(sdk_version()),
                project_root: "/repo".to_string(),
            }],
        );
        assert!(matches!(result, Err(Session::Handshake(_))));
        let WorkerMessage::Failed { message } = &replies(&output)[0] else {
            panic!("expected Failed");
        };
        assert!(
            message.contains("does not match worker protocol"),
            "{message}"
        );
    }

    #[test]
    fn a_version_skew_fails_the_handshake() {
        let pipeline = Pipeline::new();
        let (result, output) = drive(
            &pipeline,
            &[HostMessage::Hello {
                protocol: PROTOCOL_VERSION,
                host_version: "0.0.1".to_string(),
                capability_digest: capability_digest(sdk_version()),
                project_root: "/repo".to_string(),
            }],
        );
        assert!(matches!(result, Err(Session::Handshake(_))));
        let WorkerMessage::Failed { message } = &replies(&output)[0] else {
            panic!("expected Failed");
        };
        assert!(message.contains("does not match the gnr8 SDK"), "{message}");
    }

    #[test]
    fn a_capability_skew_fails_the_handshake() {
        let pipeline = Pipeline::new();
        let (result, _) = drive(
            &pipeline,
            &[HostMessage::Hello {
                protocol: PROTOCOL_VERSION,
                host_version: sdk_version().to_string(),
                capability_digest: "0".repeat(64),
                project_root: "/repo".to_string(),
            }],
        );
        assert!(matches!(result, Err(Session::Handshake(_))));
    }

    #[test]
    fn a_missing_handshake_is_a_handshake_failure_not_a_run_failure() {
        let (result, _) = drive(&Pipeline::new(), &[]);
        assert!(matches!(result, Err(Session::Handshake(_))));
    }

    #[test]
    fn a_request_for_a_builtin_position_is_a_protocol_error() {
        let pipeline = Pipeline::new().transform(crate::sdk::builtins::SetTitle::new("A"));
        let (result, output) = drive(
            &pipeline,
            &[
                hello(),
                HostMessage::ApplyTransforms {
                    indices: vec![0],
                    graph: ApiGraph::default(),
                },
            ],
        );
        assert!(matches!(result, Err(Session::Run(_))));
        let WorkerMessage::Failed { message } = &replies(&output)[1] else {
            panic!("expected Failed");
        };
        assert!(
            message.contains("no custom transform at that position"),
            "{message}"
        );
    }
}
