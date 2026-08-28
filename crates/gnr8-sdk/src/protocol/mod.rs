//! The host↔worker frame protocol.
//!
//! The installed `gnr8` host builds a project's `.gnr8/` crate once, then executes the resulting
//! binary directly and talks to it over its stdin/stdout.
//!
//! A work request names a CONSECUTIVE RUN of the user's own stages, not one stage. The host runs
//! built-ins itself, so a plan is a sequence of alternating host and worker runs, and the graph or
//! artifact set only has to cross the process boundary once per run. On a pipeline with seventeen
//! custom transforms in five runs that is five crossings instead of seventeen, and the crossing —
//! encode, pipe, decode, on both sides — is what a large graph costs.
//!
//! What crosses on a run is what CHANGED, in both directions. Each side remembers the vectors the
//! other holds — the graph's operations and schemas, and the artifact set — so an element the peer
//! still holds is named by its position there rather than serialized, piped and parsed a second
//! time ([`Patched`]). A transform that renames one operation ships that one operation; a
//! post-processor that rewrites two files of five thousand is answered with two files. On a
//! 332-artifact project that is 1.3 MB of graph across a warm run instead of 5.2 MB, and 2.2 MB of
//! artifacts instead of 7.5 MB.
//!
//! Every message is one self-delimiting, digest-checked frame:
//!
//! ```text
//! b"GN8F" | payload length: u32 big-endian | BLAKE3(payload): 32 bytes | payload: compact JSON
//! ```
//!
//! Three properties matter and each is enforced here rather than assumed:
//!
//! * **Self-delimiting.** The length prefix is read first and the payload with `read_exact`, so a
//!   truncated pipe is a typed error rather than a half-parsed message.
//! * **Bounded.** A declared length above [`MAX_FRAME_BYTES`] is rejected *before* any allocation,
//!   so a runaway peer cannot exhaust the reader's memory.
//! * **Integrity-checked.** The digest is verified before the payload is handed to serde, so silent
//!   corruption surfaces as corruption. This is an integrity check on a local pipe between two
//!   processes the user compiled — it is deliberately not, and is never described as, an
//!   authentication or sandboxing boundary.

use std::io::{Read, Write};

use std::collections::BTreeMap;

use crate::graph::{ApiGraph, Operation, Schema};
use crate::sdk::{Artifact, StagePlan};
use crate::Error;

/// The current host/worker protocol version.
///
/// Bumped on any breaking change to the frame or message shape. Both sides refuse to proceed on a
/// mismatch, so a `.gnr8/` crate built against a skewed SDK fails with an actionable error rather
/// than a confusing parse failure or silently-wrong output.
pub const PROTOCOL_VERSION: u32 = 3;

/// The frame magic. A stream that does not start with it is not this protocol.
pub const FRAME_MAGIC: [u8; 4] = *b"GN8F";

/// The largest payload either side will send or accept, in bytes.
///
/// Generated SDKs are text and the largest real payload is the full artifact set for one project;
/// 64 MiB is far above any observed pipeline and far below a memory hazard.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Deterministic fingerprint of the capabilities that must agree across the boundary.
///
/// A version match alone is not enough: two builds at the same version can still disagree if the
/// protocol or the plan shape changed underneath them. Both sides compute this from compile-time
/// constants and compare it during the handshake.
#[must_use]
pub fn capability_digest(sdk_version: &str) -> String {
    let manifest = format!(
        "gnr8-sdk:{sdk_version};protocol:{PROTOCOL_VERSION};frames:1;plan:1;artifacts:1;patched:1"
    );
    blake3::hash(manifest.as_bytes()).to_hex().to_string()
}

/// The SDK version this crate was compiled at.
#[must_use]
pub fn sdk_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A vector handed across the boundary as the difference from the one the peer already holds.
///
/// The graph's operations and schemas, and a run's artifact set, are the megabytes of a pipeline,
/// and a run typically leaves almost every element exactly as it found it. The peer holds the
/// previous vector, so an element it still holds is named by its position there instead of being
/// serialized, piped and parsed a second time; only the elements it has never seen ride along.
///
/// This is the request-side counterpart of [`WorkerMessage::ArtifactChanges`]. Both directions now
/// state what changed, and neither re-sends what the other already has.
///
/// The two sides stay in step because each transition is made by BOTH of them on the same frame:
/// the sender records what it just described as what the peer holds, and the receiver records what
/// it just rebuilt. A `Patched` is therefore always measured against a vector both sides agree on,
/// and a slot that names a position the receiver does not hold is a protocol error rather than a
/// silently wrong graph.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Patched<T> {
    /// One entry per element of the described vector, in order: `Some(position)` is the element the
    /// peer holds at `position`, `None` takes the next value from `fresh`.
    pub slots: Vec<Option<usize>>,
    /// The elements the peer does not hold, in the order the `None` slots consume them.
    pub fresh: Vec<T>,
}

impl<T: Clone + PartialEq> Patched<T> {
    /// Describe `next` against the `held` vector the peer holds, matching elements by `identity`.
    ///
    /// An element is reused only when the peer's element of the same identity is EQUAL to it, so a
    /// stage that rewrote a value in place still ships that value. Identity only narrows the search;
    /// equality decides, so duplicate or reordered identities cost bytes, never correctness.
    pub fn of<F>(next: &[T], held: &[T], identity: F) -> Self
    where
        F: for<'element> Fn(&'element T) -> &'element str,
    {
        let mut positions: BTreeMap<&str, usize> = BTreeMap::new();
        for (position, element) in held.iter().enumerate() {
            positions.entry(identity(element)).or_insert(position);
        }
        let mut slots = Vec::with_capacity(next.len());
        let mut fresh = Vec::new();
        for element in next {
            let reused = positions
                .get(identity(element))
                .copied()
                .filter(|position| held.get(*position).is_some_and(|peer| peer == element));
            if reused.is_none() {
                fresh.push(element.clone());
            }
            slots.push(reused);
        }
        Self { slots, fresh }
    }

    /// Rebuild the described vector against the `held` vector this side holds.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Protocol`] when a slot names a position this side does not hold, or when the
    /// carried elements run out before the slots do — either means the two sides disagree about the
    /// vector the patch was measured against.
    pub fn resolve(self, held: &[T]) -> Result<Vec<T>, Error> {
        let Self { slots, fresh } = self;
        let mut fresh = fresh.into_iter();
        let mut resolved = Vec::with_capacity(slots.len());
        for slot in slots {
            let element = match slot {
                Some(position) => held
                    .get(position)
                    .ok_or_else(|| {
                        Error::protocol(format!(
                            "a frame reused element {position} of a vector this side holds {}                              element(s) of; the host and worker disagree about the previous one",
                            held.len()
                        ))
                    })?
                    .clone(),
                None => fresh.next().ok_or_else(|| {
                    Error::protocol(
                        "a frame described more new elements than it carried".to_string(),
                    )
                })?,
            };
            resolved.push(element);
        }
        Ok(resolved)
    }
}

/// The graph vectors one side of the boundary holds — what a [`GraphPatch`] is measured against.
///
/// Only the operations and schemas are tracked: they are the megabytes, and the rest of a graph is
/// kilobytes of metadata that always travels whole.
#[derive(Debug, Clone, Default)]
pub struct HeldGraph {
    /// The operations, in the order the holder has them.
    pub operations: Vec<Operation>,
    /// The schemas, in the order the holder has them.
    pub schemas: Vec<Schema>,
}

impl HeldGraph {
    /// What a side holds once `graph` is its current graph.
    #[must_use]
    pub fn of(graph: &ApiGraph) -> Self {
        Self {
            operations: graph.operations.clone(),
            schemas: graph.schemas.clone(),
        }
    }

    /// The vectors of `graph`, taken rather than copied, for a caller that is done with it.
    #[must_use]
    pub fn taken_from(graph: ApiGraph) -> Self {
        Self {
            operations: graph.operations,
            schemas: graph.schemas,
        }
    }
}

/// A graph handed across the boundary as the difference from the one the peer already holds.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphPatch {
    /// The graph's own metadata, with the operation and schema vectors left empty.
    pub metadata: ApiGraph,
    /// The operations, against the ones the peer holds.
    pub operations: Patched<Operation>,
    /// The schemas, against the ones the peer holds.
    pub schemas: Patched<Schema>,
}

impl GraphPatch {
    /// Describe `next` against the graph the peer holds.
    ///
    /// `next` is borrowed mutably only to lift its two large vectors out of the way while the
    /// metadata is copied — copying the graph and then clearing them would copy the megabytes this
    /// exists to avoid. It is left exactly as it was found.
    #[must_use]
    pub fn of(next: &mut ApiGraph, held: &HeldGraph) -> Self {
        let operations = Patched::of(&next.operations, &held.operations, |operation| {
            operation.id.as_str()
        });
        let schemas = Patched::of(&next.schemas, &held.schemas, |schema| schema.id.as_str());
        let lifted_operations = std::mem::take(&mut next.operations);
        let lifted_schemas = std::mem::take(&mut next.schemas);
        let metadata = next.clone();
        next.operations = lifted_operations;
        next.schemas = lifted_schemas;
        Self {
            metadata,
            operations,
            schemas,
        }
    }

    /// Rebuild the graph this patch describes against the one this side holds.
    ///
    /// # Errors
    ///
    /// Propagates [`Patched::resolve`]'s protocol error.
    pub fn resolve(self, held: &HeldGraph) -> Result<ApiGraph, Error> {
        let Self {
            mut metadata,
            operations,
            schemas,
        } = self;
        metadata.operations = operations.resolve(&held.operations)?;
        metadata.schemas = schemas.resolve(&held.schemas)?;
        Ok(metadata)
    }
}

/// A message the host sends to the worker.
///
/// One value exists at a time and is serialized immediately, so the size spread between a bare
/// `Shutdown` and a full graph payload costs nothing worth boxing around.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostMessage {
    /// Open the session. Always the first frame.
    Hello {
        /// The host's [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The host CLI's exact version.
        host_version: String,
        /// The host's [`capability_digest`].
        capability_digest: String,
        /// The project root the host resolved, for the worker's [`crate::sdk::Cx`].
        project_root: String,
    },
    /// Run the custom source at `index` and return its graph.
    LoadSource {
        /// Position within the pipeline's source vector.
        index: usize,
    },
    /// Run the custom transforms at `indices`, in order, over `graph`.
    ApplyTransforms {
        /// Positions within the pipeline's transform vector, in composition order.
        indices: Vec<usize>,
        /// The graph as it stands after every earlier stage, against the one the worker holds.
        graph: GraphPatch,
    },
    /// Hand over the frozen, generation-ready graph every target sees.
    ///
    /// Sent once, before the first target run: the graph is frozen for the whole target phase, so
    /// re-sending it with each run would ship the same megabytes again for no new fact.
    FreezeGraph {
        /// The frozen, generation-ready graph, against the one the worker holds.
        graph: GraphPatch,
    },
    /// Run the custom targets at `indices`, in order, and return the artifact set they produced.
    GenerateTargets {
        /// Positions within the pipeline's target vector, in composition order.
        indices: Vec<usize>,
        /// The artifact set as it stands after every earlier target, against the one the worker
        /// holds.
        artifacts: Patched<Artifact>,
    },
    /// Run the custom post-processors at `indices`, in order, over `artifacts`.
    RunPosts {
        /// Positions within the pipeline's post vector, in composition order.
        indices: Vec<usize>,
        /// The artifact set as it stands after every earlier stage, against the one the worker
        /// holds.
        artifacts: Patched<Artifact>,
    },
    /// End the session. The worker exits 0 after acknowledging.
    Shutdown,
}

/// A message the worker sends to the host.
///
/// As with [`HostMessage`], one value exists at a time and is serialized immediately.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerMessage {
    /// Handshake accepted; here is the composed pipeline.
    Ready {
        /// The worker's [`PROTOCOL_VERSION`].
        protocol: u32,
        /// The linked SDK's exact version.
        sdk_version: String,
        /// The worker's [`capability_digest`].
        capability_digest: String,
        /// The ordered stage plan.
        plan: StagePlan,
    },
    /// The result of a source or transform request.
    Graph {
        /// The produced or mutated graph, against the one the host holds.
        graph: GraphPatch,
    },
    /// The result of a target or post-process request: what the run CHANGED.
    ///
    /// The host already holds the set it sent, so shipping it back would re-encode and re-decode
    /// megabytes to say "unchanged" — a post-processor that rewrites two files of five thousand paid
    /// for all five thousand. The reply is therefore the artifacts the run created or altered, and
    /// the host merges them into the set it kept.
    ///
    /// This is also what makes "a stage may create, overlay or rewrite an artifact but never drop
    /// one" true of the WIRE and not just of the process: an additive reply cannot express a drop.
    ArtifactChanges {
        /// The artifacts the run created or changed, sorted by path.
        changed: Vec<Artifact>,
    },
    /// Acknowledgement of [`HostMessage::Shutdown`].
    Done,
    /// The worker could not satisfy the request.
    Failed {
        /// The stage's own error text.
        message: String,
    },
}

/// Serialize `message` and write it as one frame.
///
/// # Errors
///
/// Returns [`Error::Protocol`] when the payload cannot be serialized, exceeds
/// [`MAX_FRAME_BYTES`], or cannot be written.
pub fn write_frame<W: Write>(writer: &mut W, message: &impl serde::Serialize) -> Result<(), Error> {
    let payload = serde_json::to_vec(message)
        .map_err(|err| Error::protocol(format!("failed to serialize a frame payload: {err}")))?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(Error::protocol(format!(
            "refusing to send a {} byte frame; the limit is {MAX_FRAME_BYTES} bytes",
            payload.len()
        )));
    }
    let len = u32::try_from(payload.len())
        .map_err(|_| Error::protocol("frame payload length does not fit in u32".to_string()))?;
    let digest = blake3::hash(&payload);
    writer
        .write_all(&FRAME_MAGIC)
        .and_then(|()| writer.write_all(&len.to_be_bytes()))
        .and_then(|()| writer.write_all(digest.as_bytes()))
        .and_then(|()| writer.write_all(&payload))
        .and_then(|()| writer.flush())
        .map_err(|err| Error::protocol(format!("failed to write a frame: {err}")))
}

/// Read one frame and deserialize its payload.
///
/// # Errors
///
/// Returns [`Error::Protocol`] when the stream ends, the magic is wrong, the declared length
/// exceeds [`MAX_FRAME_BYTES`], the digest does not match, or the payload is not the expected
/// message.
pub fn read_frame<R: Read, T: serde::de::DeserializeOwned>(reader: &mut R) -> Result<T, Error> {
    let mut magic = [0u8; 4];
    read_exact(reader, &mut magic, "frame magic")?;
    if magic != FRAME_MAGIC {
        return Err(Error::protocol(format!(
            "expected a gnr8 protocol frame, got magic {magic:?}; the peer is not a gnr8 worker \
             or wrote to stdout directly"
        )));
    }
    let mut len_bytes = [0u8; 4];
    read_exact(reader, &mut len_bytes, "frame length")?;
    let len = u32::from_be_bytes(len_bytes) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(Error::protocol(format!(
            "frame declares {len} bytes, above the {MAX_FRAME_BYTES} byte limit"
        )));
    }
    let mut digest = [0u8; 32];
    read_exact(reader, &mut digest, "frame digest")?;
    let mut payload = vec![0u8; len];
    read_exact(reader, &mut payload, "frame payload")?;
    let actual = blake3::hash(&payload);
    if actual.as_bytes() != &digest {
        return Err(Error::protocol(
            "frame digest mismatch; the payload was truncated or corrupted in transit".to_string(),
        ));
    }
    serde_json::from_slice(&payload)
        .map_err(|err| Error::protocol(format!("failed to parse a frame payload: {err}")))
}

fn read_exact<R: Read>(reader: &mut R, buf: &mut [u8], what: &str) -> Result<(), Error> {
    reader.read_exact(buf).map_err(|err| {
        if err.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::protocol(format!("the stream ended while reading the {what}"))
        } else {
            Error::protocol(format!("failed to read the {what}: {err}"))
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{
        capability_digest, read_frame, write_frame, GraphPatch, HeldGraph, HostMessage, Patched,
        WorkerMessage, FRAME_MAGIC, MAX_FRAME_BYTES, PROTOCOL_VERSION,
    };
    use crate::graph::{ApiGraph, Schema};
    use crate::sdk::Artifact;

    fn artifact(path: &str, text: &str) -> Artifact {
        Artifact::new(path, text)
    }

    fn patch(next: &[Artifact], held: &[Artifact]) -> Patched<Artifact> {
        Patched::of(next, held, |artifact| artifact.path.as_str())
    }

    #[test]
    fn an_element_the_peer_holds_is_named_rather_than_sent() {
        let held = vec![artifact("a.txt", "a"), artifact("b.txt", "b")];
        let next = vec![artifact("a.txt", "a"), artifact("b.txt", "changed")];
        let described = patch(&next, &held);
        assert_eq!(described.slots, vec![Some(0), None]);
        assert_eq!(described.fresh.len(), 1, "only the changed one rides along");
        assert_eq!(described.fresh[0].path, "b.txt");
        assert_eq!(described.resolve(&held).unwrap(), next);
    }

    #[test]
    fn an_insert_a_removal_and_a_reorder_all_rebuild_exactly() {
        let held = vec![
            artifact("a.txt", "a"),
            artifact("b.txt", "b"),
            artifact("c.txt", "c"),
        ];
        // `b` is gone, `c` and `a` swapped, and `d` is new.
        let next = vec![
            artifact("c.txt", "c"),
            artifact("a.txt", "a"),
            artifact("d.txt", "d"),
        ];
        let described = patch(&next, &held);
        assert_eq!(described.slots, vec![Some(2), Some(0), None]);
        assert_eq!(described.resolve(&held).unwrap(), next);
    }

    #[test]
    fn a_peer_that_holds_nothing_receives_every_element() {
        let next = vec![artifact("a.txt", "a"), artifact("b.txt", "b")];
        let described = patch(&next, &[]);
        assert_eq!(described.slots, vec![None, None]);
        assert_eq!(described.resolve(&[]).unwrap(), next);
    }

    /// Identity only narrows the search; equality decides. A duplicated path whose two values differ
    /// must still rebuild both, not collapse them onto the first match.
    #[test]
    fn a_duplicated_identity_costs_bytes_never_correctness() {
        let held = vec![artifact("a.txt", "one"), artifact("a.txt", "two")];
        let next = vec![artifact("a.txt", "two"), artifact("a.txt", "one")];
        let described = patch(&next, &held);
        assert_eq!(described.resolve(&held).unwrap(), next);
    }

    /// A slot naming a position the receiver does not hold means the two sides disagree about the
    /// previous vector — a typed protocol error, never a silently different set.
    #[test]
    fn a_slot_the_receiver_does_not_hold_is_a_protocol_error() {
        let described: Patched<Artifact> = Patched {
            slots: vec![Some(7)],
            fresh: Vec::new(),
        };
        let err = described.resolve(&[artifact("a.txt", "a")]).unwrap_err();
        assert!(err.to_string().contains("disagree"), "{err}");
    }

    #[test]
    fn a_patch_that_carries_too_few_elements_is_a_protocol_error() {
        let described: Patched<Artifact> = Patched {
            slots: vec![None, None],
            fresh: vec![artifact("a.txt", "a")],
        };
        let err = described.resolve(&[]).unwrap_err();
        assert!(err.to_string().contains("more new elements"), "{err}");
    }

    fn schema(id: &str) -> Schema {
        Schema {
            id: id.to_string(),
            name: id.to_string(),
            body: crate::facts::Type::Primitive(crate::facts::Prim::String),
            enum_source_order: Vec::new(),
            provenance: crate::graph::SourceSpan {
                file: format!("{id}.go"),
                start_line: 1,
                end_line: 2,
            },
        }
    }

    #[test]
    fn a_graph_patch_leaves_the_graph_it_described_untouched_and_rebuilds_it() {
        let graph = ApiGraph {
            title: "Bookstore".to_string(),
            schemas: vec![schema("Book"), schema("Author")],
            ..ApiGraph::default()
        };
        let held = HeldGraph::of(&graph);
        let mut next = ApiGraph {
            title: "Bookstore v2".to_string(),
            ..graph.clone()
        };
        let described = GraphPatch::of(&mut next, &held);
        assert_eq!(
            next,
            ApiGraph {
                title: "Bookstore v2".to_string(),
                ..graph.clone()
            }
        );
        assert!(
            described.schemas.fresh.is_empty(),
            "an untouched schema must not ride along with a metadata change"
        );
        assert!(
            described.metadata.schemas.is_empty(),
            "the metadata half of a patch carries no schemas"
        );
        assert_eq!(described.resolve(&held).unwrap(), next);
    }

    /// The frame is what actually crosses, so the saving has to survive encoding — not just the
    /// in-memory patch.
    #[test]
    fn an_unchanged_graph_crosses_as_a_fraction_of_itself() {
        let graph = ApiGraph {
            schemas: (0..200).map(|n| schema(&format!("Schema{n}"))).collect(),
            ..ApiGraph::default()
        };
        let held = HeldGraph::of(&graph);
        let mut next = graph.clone();
        let whole = {
            let mut bytes = Vec::new();
            write_frame(
                &mut bytes,
                &WorkerMessage::Graph {
                    graph: GraphPatch::of(&mut next.clone(), &HeldGraph::default()),
                },
            )
            .unwrap();
            bytes.len()
        };
        let mut patched = Vec::new();
        write_frame(
            &mut patched,
            &WorkerMessage::Graph {
                graph: GraphPatch::of(&mut next, &held),
            },
        )
        .unwrap();
        assert!(
            patched.len() * 4 < whole,
            "an unchanged 200-schema graph crossed as {} bytes against {whole} whole",
            patched.len()
        );
    }

    fn encoded(message: &HostMessage) -> Vec<u8> {
        let mut buf = Vec::new();
        write_frame(&mut buf, message).unwrap();
        buf
    }

    #[test]
    fn a_frame_round_trips() {
        let message = HostMessage::Hello {
            protocol: PROTOCOL_VERSION,
            host_version: "0.9.0".to_string(),
            capability_digest: capability_digest("0.9.0"),
            project_root: "/repo".to_string(),
        };
        let bytes = encoded(&message);
        assert_eq!(&bytes[..4], &FRAME_MAGIC);
        let back: HostMessage = read_frame(&mut bytes.as_slice()).unwrap();
        let HostMessage::Hello { host_version, .. } = back else {
            panic!("expected Hello");
        };
        assert_eq!(host_version, "0.9.0");
    }

    #[test]
    fn several_frames_stream_back_to_back() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &HostMessage::LoadSource { index: 0 }).unwrap();
        write_frame(&mut buf, &HostMessage::Shutdown).unwrap();
        let mut cursor = buf.as_slice();
        assert!(matches!(
            read_frame::<_, HostMessage>(&mut cursor).unwrap(),
            HostMessage::LoadSource { index: 0 }
        ));
        assert!(matches!(
            read_frame::<_, HostMessage>(&mut cursor).unwrap(),
            HostMessage::Shutdown
        ));
    }

    #[test]
    fn a_bad_magic_is_rejected_before_anything_is_parsed() {
        let mut bytes = encoded(&HostMessage::Shutdown);
        bytes[0] = b'X';
        let err = read_frame::<_, HostMessage>(&mut bytes.as_slice()).unwrap_err();
        assert!(
            err.to_string().contains("expected a gnr8 protocol frame"),
            "{err}"
        );
    }

    #[test]
    fn a_corrupted_payload_fails_the_digest_check() {
        let mut bytes = encoded(&HostMessage::LoadSource { index: 7 });
        let last = bytes.len() - 2;
        bytes[last] = b'0';
        let err = read_frame::<_, HostMessage>(&mut bytes.as_slice()).unwrap_err();
        assert!(err.to_string().contains("digest mismatch"), "{err}");
    }

    #[test]
    fn an_oversize_declared_length_is_rejected_without_allocating() {
        let mut bytes = encoded(&HostMessage::Shutdown);
        let oversize = u32::try_from(MAX_FRAME_BYTES + 1).unwrap();
        bytes[4..8].copy_from_slice(&oversize.to_be_bytes());
        let err = read_frame::<_, HostMessage>(&mut bytes.as_slice()).unwrap_err();
        assert!(err.to_string().contains("above the"), "{err}");
    }

    #[test]
    fn a_truncated_frame_is_an_error_not_a_partial_parse() {
        let bytes = encoded(&HostMessage::LoadSource { index: 3 });
        let truncated = &bytes[..bytes.len() - 3];
        let err = read_frame::<_, HostMessage>(&mut { truncated }).unwrap_err();
        assert!(err.to_string().contains("stream ended"), "{err}");
    }

    #[test]
    fn an_empty_stream_reports_the_missing_frame() {
        let err = read_frame::<_, HostMessage>(&mut [].as_slice()).unwrap_err();
        assert!(err.to_string().contains("stream ended"), "{err}");
    }

    #[test]
    fn worker_messages_carry_their_discriminant() {
        let mut buf = Vec::new();
        write_frame(
            &mut buf,
            &WorkerMessage::Failed {
                message: "boom".to_string(),
            },
        )
        .unwrap();
        let back: WorkerMessage = read_frame(&mut buf.as_slice()).unwrap();
        let WorkerMessage::Failed { message } = back else {
            panic!("expected Failed");
        };
        assert_eq!(message, "boom");
    }

    #[test]
    fn the_capability_digest_moves_with_the_version() {
        assert_ne!(capability_digest("0.9.0"), capability_digest("0.9.1"));
        assert_eq!(capability_digest("0.9.0"), capability_digest("0.9.0"));
    }
}
