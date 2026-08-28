//! The host↔worker frame protocol.
//!
//! The installed `gnr8` host builds a project's `.gnr8/` crate once, then executes the resulting
//! binary directly and talks to it over its stdin/stdout. Every message is one self-delimiting,
//! digest-checked frame:
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

use crate::graph::ApiGraph;
use crate::sdk::{Artifact, StagePlan};
use crate::Error;

/// The current host/worker protocol version.
///
/// Bumped on any breaking change to the frame or message shape. Both sides refuse to proceed on a
/// mismatch, so a `.gnr8/` crate built against a skewed SDK fails with an actionable error rather
/// than a confusing parse failure or silently-wrong output.
pub const PROTOCOL_VERSION: u32 = 1;

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
    let manifest =
        format!("gnr8-sdk:{sdk_version};protocol:{PROTOCOL_VERSION};frames:1;plan:1;artifacts:1");
    blake3::hash(manifest.as_bytes()).to_hex().to_string()
}

/// The SDK version this crate was compiled at.
#[must_use]
pub fn sdk_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
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
    /// Run the custom transform at `index` over `graph` and return the mutated graph.
    ApplyTransform {
        /// Position within the pipeline's transform vector.
        index: usize,
        /// The graph as it stands after every earlier stage.
        graph: ApiGraph,
    },
    /// Run the custom target at `index` and return the artifact set it produced.
    GenerateTarget {
        /// Position within the pipeline's target vector.
        index: usize,
        /// The frozen, generation-ready graph.
        graph: ApiGraph,
        /// The artifact set as it stands after every earlier target.
        artifacts: Vec<Artifact>,
    },
    /// Run the custom post-processor at `index` over `artifacts`.
    RunPost {
        /// Position within the pipeline's post vector.
        index: usize,
        /// The artifact set as it stands after every earlier stage.
        artifacts: Vec<Artifact>,
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
        /// The produced or mutated graph.
        graph: ApiGraph,
    },
    /// The result of a target or post-process request.
    Artifacts {
        /// The artifact set after the stage ran.
        artifacts: Vec<Artifact>,
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
        capability_digest, read_frame, write_frame, HostMessage, WorkerMessage, FRAME_MAGIC,
        MAX_FRAME_BYTES, PROTOCOL_VERSION,
    };

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
