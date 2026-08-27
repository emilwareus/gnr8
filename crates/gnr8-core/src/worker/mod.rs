//! The host side of the worker boundary: build it, start it, talk to it, stop it.
//!
//! [`build`] turns a project's `.gnr8/` crate into an executable (or proves the existing one is
//! still current). [`WorkerSession`] runs that executable with piped stdio and drives the frame
//! protocol, implementing [`crate::pipeline::StageRunner`] so the host's pipeline can call back into
//! the user's own stages.
//!
//! ## Bounds
//!
//! | Bound | Value |
//! |---|---|
//! | frame size | [`gnr8::protocol::MAX_FRAME_BYTES`], checked before allocation |
//! | stderr kept | [`STDERR_CAPTURE_BYTES`], then drained and marked truncated |
//! | wall clock | [`DEFAULT_TIMEOUT_SECS`], overridable with `GNR8_WORKER_TIMEOUT_SECS` |
//! | process tree | the direct worker process only |
//!
//! That last row is a real limitation, stated rather than glossed: the workspace forbids `unsafe`,
//! so gnr8 cannot put the worker in its own process group, and a grandchild the user's own stage
//! spawns is not tracked. Killing the worker closes its pipes, which is what unblocks the host.

pub mod build;

use std::io::{BufReader, Read};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use gnr8::protocol::{
    capability_digest, read_frame, sdk_version, write_frame, HostMessage, WorkerMessage,
    PROTOCOL_VERSION,
};

use crate::graph::ApiGraph;
use crate::pipeline::StageRunner;
use crate::sdk::{Artifact, Cx, StagePlan};
use crate::CoreError;

pub use build::{
    discard_stamp, ensure_worker, validate_workspace, WorkerBinary, WorkerPolicy, Workspace,
};

/// How much worker stderr is retained for an error message before truncation.
pub const STDERR_CAPTURE_BYTES: usize = 1024 * 1024;

/// Default wall-clock budget for one worker session.
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Override for [`DEFAULT_TIMEOUT_SECS`], in whole seconds. `0` disables the watchdog.
pub const GNR8_WORKER_TIMEOUT_ENV: &str = "GNR8_WORKER_TIMEOUT_SECS";

/// The wall-clock budget for one session.
///
/// A malformed override is an error rather than a silent return to the default: a caller who set the
/// variable meant something by it, and quietly ignoring it would hide a typo behind a 300-second wait.
fn session_timeout() -> Result<Option<Duration>, CoreError> {
    let secs = match std::env::var(GNR8_WORKER_TIMEOUT_ENV) {
        Ok(value) => value.parse::<u64>().map_err(|_| CoreError::WorkerRun {
            message: format!(
                "{GNR8_WORKER_TIMEOUT_ENV}={value:?} is not a whole number of seconds (0 disables \
                 the watchdog)"
            ),
        })?,
        Err(_) => DEFAULT_TIMEOUT_SECS,
    };
    Ok((secs > 0).then(|| Duration::from_secs(secs)))
}

/// Bounded collector for the worker's stderr.
#[derive(Default)]
struct StderrBuffer {
    bytes: Vec<u8>,
    truncated: bool,
}

impl StderrBuffer {
    fn render(&self) -> String {
        let mut text = String::from_utf8_lossy(&self.bytes).trim_end().to_string();
        if self.truncated {
            text.push_str("\n… worker stderr truncated at ");
            text.push_str(&STDERR_CAPTURE_BYTES.to_string());
            text.push_str(" bytes");
        }
        text
    }
}

/// Kills the worker if the session outlives its budget.
struct Watchdog {
    finished: Arc<(Mutex<bool>, Condvar)>,
    fired: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    budget: Option<Duration>,
}

impl Watchdog {
    fn start(child: Arc<Mutex<Child>>, timeout: Option<Duration>) -> Self {
        let finished = Arc::new((Mutex::new(false), Condvar::new()));
        let fired = Arc::new(AtomicBool::new(false));
        let Some(timeout) = timeout else {
            return Self {
                finished,
                fired,
                handle: None,
                budget: None,
            };
        };
        let watch_finished = Arc::clone(&finished);
        let watch_fired = Arc::clone(&fired);
        let handle = std::thread::spawn(move || {
            let (lock, condvar) = &*watch_finished;
            let Ok(guard) = lock.lock() else {
                return;
            };
            let Ok((guard, timeout_result)) =
                condvar.wait_timeout_while(guard, timeout, |done| !*done)
            else {
                return;
            };
            drop(guard);
            if timeout_result.timed_out() {
                watch_fired.store(true, Ordering::SeqCst);
                if let Ok(mut child) = child.lock() {
                    let _ = child.kill();
                }
            }
        });
        Self {
            finished,
            fired,
            handle: Some(handle),
            budget: Some(timeout),
        }
    }

    fn stop(&mut self) {
        let (lock, condvar) = &*self.finished;
        if let Ok(mut done) = lock.lock() {
            *done = true;
        }
        condvar.notify_all();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    fn timed_out(&self) -> bool {
        self.fired.load(Ordering::SeqCst)
    }

    fn budget_secs(&self) -> u64 {
        self.budget.map_or(0, |budget| budget.as_secs())
    }
}

/// A running worker process and the plan it reported.
pub struct WorkerSession {
    child: Arc<Mutex<Child>>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    stderr: Arc<Mutex<StderrBuffer>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
    watchdog: Watchdog,
    plan: StagePlan,
    finished: bool,
    built: bool,
}

impl WorkerSession {
    /// Build (if needed) and start the worker for `project_root`, completing the handshake.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::WorkerBuild`] for workspace/build failures and
    /// [`CoreError::WorkerRun`] for spawn, handshake, or protocol failures.
    pub fn start(project_root: &std::path::Path, policy: WorkerPolicy) -> Result<Self, CoreError> {
        let workspace = validate_workspace(project_root)?;
        if !policy.allow_execute {
            return Err(CoreError::WorkerRun {
                message: format!(
                    "running the .gnr8 worker for {} was refused. Compiling and running .gnr8/ \
                     executes Rust from this repository with your privileges; re-run without \
                     --no-execute to allow it.",
                    project_root.display()
                ),
            });
        }
        let binary = ensure_worker(&workspace, policy)?;
        let mut session = Self::start_binary(&workspace, &binary.path)?;
        session.built = binary.built;
        Ok(session)
    }

    /// Start an already-built worker binary and complete the handshake.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::WorkerRun`] when the process cannot be spawned or the handshake fails.
    pub fn start_binary(
        workspace: &Workspace,
        binary: &std::path::Path,
    ) -> Result<Self, CoreError> {
        let mut command = Command::new(binary);
        command
            .current_dir(&workspace.project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|err| CoreError::WorkerRun {
            message: format!(
                "failed to start the .gnr8 worker {}: {err}",
                binary.display()
            ),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| CoreError::WorkerRun {
            message: "the .gnr8 worker was started without a stdin pipe".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| CoreError::WorkerRun {
            message: "the .gnr8 worker was started without a stdout pipe".to_string(),
        })?;
        let child_stderr = child.stderr.take().ok_or_else(|| CoreError::WorkerRun {
            message: "the .gnr8 worker was started without a stderr pipe".to_string(),
        })?;

        let stderr = Arc::new(Mutex::new(StderrBuffer::default()));
        let drain = Arc::clone(&stderr);
        let stderr_thread = std::thread::spawn(move || drain_stderr(child_stderr, &drain));

        let timeout = session_timeout()?;
        let child = Arc::new(Mutex::new(child));
        let watchdog = Watchdog::start(Arc::clone(&child), timeout);

        let mut session = Self {
            child,
            stdin: Some(stdin),
            stdout: Some(BufReader::new(stdout)),
            stderr,
            stderr_thread: Some(stderr_thread),
            watchdog,
            plan: StagePlan::default(),
            finished: false,
            built: false,
        };
        session.plan = session.handshake(&workspace.project_root)?;
        Ok(session)
    }

    fn handshake(&mut self, project_root: &std::path::Path) -> Result<StagePlan, CoreError> {
        let hello = HostMessage::Hello {
            protocol: PROTOCOL_VERSION,
            host_version: sdk_version().to_string(),
            capability_digest: capability_digest(sdk_version()),
            project_root: project_root.to_string_lossy().into_owned(),
        };
        self.send(&hello)?;
        match self.receive()? {
            WorkerMessage::Ready {
                protocol,
                sdk_version: worker_sdk,
                capability_digest: worker_digest,
                plan,
            } => {
                if protocol != PROTOCOL_VERSION {
                    return Err(Self::protocol_error(format!(
                        "the .gnr8 worker speaks protocol {protocol}, but this gnr8 speaks \
                         {PROTOCOL_VERSION}. Align the installed CLI with .gnr8/Cargo.toml."
                    )));
                }
                if worker_sdk != sdk_version() {
                    return Err(Self::protocol_error(format!(
                        "gnr8 version mismatch: host {}, worker SDK {worker_sdk}. Pin the exact \
                         version in .gnr8/Cargo.toml.",
                        sdk_version()
                    )));
                }
                if worker_digest != capability_digest(sdk_version()) {
                    return Err(Self::protocol_error(format!(
                        "gnr8 capability mismatch: host {}, worker {worker_digest}. Rebuild both \
                         sides at one exact version.",
                        capability_digest(sdk_version())
                    )));
                }
                Ok(plan)
            }
            WorkerMessage::Failed { message } => Err(self.worker_error(&message)),
            other => Err(Self::protocol_error(format!(
                "the .gnr8 worker answered the handshake with {}, not its stage plan",
                message_name(&other)
            ))),
        }
    }

    /// The plan the worker reported at handshake time.
    #[must_use]
    pub fn plan(&self) -> &StagePlan {
        &self.plan
    }

    /// Whether `cargo` was invoked to produce the binary this session is running.
    #[must_use]
    pub const fn worker_built(&self) -> bool {
        self.built
    }

    /// Ask the worker to exit, then reap it.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::WorkerRun`] when the worker does not acknowledge or exits non-zero.
    pub fn shutdown(mut self) -> Result<(), CoreError> {
        let result = self.shutdown_inner();
        self.finish();
        result
    }

    fn shutdown_inner(&mut self) -> Result<(), CoreError> {
        self.send(&HostMessage::Shutdown)?;
        match self.receive()? {
            WorkerMessage::Done => Ok(()),
            WorkerMessage::Failed { message } => Err(self.worker_error(&message)),
            other => Err(Self::protocol_error(format!(
                "the .gnr8 worker answered shutdown with {}",
                message_name(&other)
            ))),
        }
    }

    fn finish(&mut self) {
        // Dropping stdin closes the worker's input, so a worker still blocked on a read exits.
        self.stdin = None;
        self.stdout = None;
        self.watchdog.stop();
        if let Ok(mut child) = self.child.lock() {
            let _ = child.wait();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
        self.finished = true;
    }

    fn send(&mut self, message: &HostMessage) -> Result<(), CoreError> {
        let stdin = self.stdin.as_mut().ok_or_else(|| CoreError::WorkerRun {
            message: "the .gnr8 worker session is already closed".to_string(),
        })?;
        write_frame(stdin, message).map_err(|err| self.transport_error(&err.to_string()))
    }

    fn receive(&mut self) -> Result<WorkerMessage, CoreError> {
        let stdout = self.stdout.as_mut().ok_or_else(|| CoreError::WorkerRun {
            message: "the .gnr8 worker session is already closed".to_string(),
        })?;
        match read_frame::<_, WorkerMessage>(stdout) {
            Ok(message) => Ok(message),
            Err(err) => Err(self.transport_error(&err.to_string())),
        }
    }

    fn stderr_text(&self) -> String {
        self.stderr
            .lock()
            .map(|buffer| buffer.render())
            .unwrap_or_default()
    }

    fn transport_error(&self, detail: &str) -> CoreError {
        if self.watchdog.timed_out() {
            return CoreError::WorkerRun {
                message: format!(
                    "the .gnr8 worker exceeded its {} second budget and was stopped. Raise \
                     ${GNR8_WORKER_TIMEOUT_ENV} if a stage legitimately takes longer. Note that \
                     any process your stage started itself is not tracked by gnr8.",
                    self.watchdog.budget_secs()
                ),
            };
        }
        let stderr = self.stderr_text();
        CoreError::WorkerRun {
            message: if stderr.is_empty() {
                format!("the .gnr8 worker connection failed: {detail}")
            } else {
                format!("the .gnr8 worker connection failed: {detail}\nWorker stderr:\n{stderr}")
            },
        }
    }

    fn protocol_error(message: String) -> CoreError {
        CoreError::Protocol { message }
    }

    fn worker_error(&self, message: &str) -> CoreError {
        let stderr = self.stderr_text();
        CoreError::WorkerRun {
            message: if stderr.is_empty() {
                message.to_string()
            } else {
                format!("{message}\nWorker stderr:\n{stderr}")
            },
        }
    }

    fn expect_graph(&mut self, request: &HostMessage) -> Result<ApiGraph, CoreError> {
        self.send(request)?;
        match self.receive()? {
            WorkerMessage::Graph { graph } => Ok(graph),
            WorkerMessage::Failed { message } => Err(self.worker_error(&message)),
            other => Err(Self::protocol_error(format!(
                "expected a graph from the .gnr8 worker, got {}",
                message_name(&other)
            ))),
        }
    }

    fn expect_artifacts(&mut self, request: &HostMessage) -> Result<Vec<Artifact>, CoreError> {
        self.send(request)?;
        match self.receive()? {
            WorkerMessage::Artifacts { artifacts } => Ok(artifacts),
            WorkerMessage::Failed { message } => Err(self.worker_error(&message)),
            other => Err(Self::protocol_error(format!(
                "expected artifacts from the .gnr8 worker, got {}",
                message_name(&other)
            ))),
        }
    }
}

impl Drop for WorkerSession {
    fn drop(&mut self) {
        if !self.finished {
            self.finish();
        }
    }
}

impl StageRunner for WorkerSession {
    fn load_source(&mut self, index: usize) -> Result<ApiGraph, CoreError> {
        self.expect_graph(&HostMessage::LoadSource { index })
    }

    fn apply_transform(&mut self, index: usize, graph: ApiGraph) -> Result<ApiGraph, CoreError> {
        self.expect_graph(&HostMessage::ApplyTransform { index, graph })
    }

    fn generate_target(
        &mut self,
        index: usize,
        graph: &ApiGraph,
        artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError> {
        self.expect_artifacts(&HostMessage::GenerateTarget {
            index,
            graph: graph.clone(),
            artifacts,
        })
    }

    fn run_post(
        &mut self,
        index: usize,
        artifacts: Vec<Artifact>,
    ) -> Result<Vec<Artifact>, CoreError> {
        self.expect_artifacts(&HostMessage::RunPost { index, artifacts })
    }
}

fn message_name(message: &WorkerMessage) -> &'static str {
    match message {
        WorkerMessage::Ready { .. } => "a handshake",
        WorkerMessage::Graph { .. } => "a graph",
        WorkerMessage::Artifacts { .. } => "artifacts",
        WorkerMessage::Done => "a shutdown acknowledgement",
        WorkerMessage::Failed { .. } => "a failure",
    }
}

/// Read the worker's stderr to EOF, keeping at most [`STDERR_CAPTURE_BYTES`].
///
/// The rest is read and dropped rather than left unread, so a chatty worker cannot deadlock on a
/// full pipe.
fn drain_stderr(mut stream: std::process::ChildStderr, buffer: &Arc<Mutex<StderrBuffer>>) {
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                let Ok(mut buffer) = buffer.lock() else {
                    return;
                };
                let room = STDERR_CAPTURE_BYTES.saturating_sub(buffer.bytes.len());
                if room == 0 {
                    buffer.truncated = true;
                } else {
                    let take = room.min(read);
                    buffer.bytes.extend_from_slice(&chunk[..take]);
                    if take < read {
                        buffer.truncated = true;
                    }
                }
            }
        }
    }
}

/// One complete pipeline run plus how its worker was obtained.
#[derive(Debug, Clone)]
pub struct PipelineRun {
    /// What the pipeline produced.
    pub outcome: crate::pipeline::PipelineOutcome,
    /// Whether `cargo` was invoked for this run.
    pub worker_built: bool,
}

/// Run a complete generation for `project_root`: build/start the worker, run the plan, stop.
///
/// # Errors
///
/// Propagates workspace, build, protocol, and stage failures.
pub fn run_pipeline(
    project_root: &std::path::Path,
    policy: WorkerPolicy,
) -> Result<PipelineRun, CoreError> {
    let mut session = WorkerSession::start(project_root, policy)?;
    let plan = session.plan().clone();
    let worker_built = session.worker_built();
    let cx = Cx::new(project_root.to_path_buf());
    let outcome = crate::pipeline::run(&plan, &cx, &mut session)?;
    session.shutdown()?;
    Ok(PipelineRun {
        outcome,
        worker_built,
    })
}

/// Run a project's pipeline through transforms only and return the graph (`gnr8 inspect`).
///
/// # Errors
///
/// Propagates workspace, build, protocol, and stage failures.
pub fn inspect_pipeline(
    project_root: &std::path::Path,
    policy: WorkerPolicy,
) -> Result<ApiGraph, CoreError> {
    let mut session = WorkerSession::start(project_root, policy)?;
    let plan = session.plan().clone();
    let cx = Cx::new(project_root.to_path_buf());
    let graph = crate::pipeline::build_ir(&plan, &cx, &mut session)?;
    session.shutdown()?;
    Ok(graph)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{StderrBuffer, STDERR_CAPTURE_BYTES};

    #[test]
    fn captured_stderr_is_bounded_and_says_so() {
        let mut buffer = StderrBuffer::default();
        buffer.bytes.extend(std::iter::repeat_n(b'x', 16));
        assert_eq!(buffer.render(), "x".repeat(16));
        buffer.truncated = true;
        let rendered = buffer.render();
        assert!(rendered.starts_with(&"x".repeat(16)));
        assert!(rendered.contains(&STDERR_CAPTURE_BYTES.to_string()));
    }
}
