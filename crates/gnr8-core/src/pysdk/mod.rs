//! Python SDK generation seam (Phase 3): generates a dependency-free Python SDK from the API graph.
//!
//! [`generate`] (filled in by Task 3) turns the Phase-2 [`crate::graph::ApiGraph`] into a single
//! deterministic, dependency-free Python SDK bundle String (D-06): an `__init__.py` re-export surface, a
//! `client.py` (an injectable `urllib.request.OpenerDirector`-backed `Client`), a typed `errors.py`
//! (`ApiError`), and a `models.py` (`@dataclass` request/response models + `enum.Enum` named enums).
//!
//! This is the structural twin of [`crate::gosdk`], MINUS the `gofmt` normalization step: Python has no
//! stdlib formatter, so [`emit`] produces already-correct significant-whitespace Python directly. Each
//! file is framed into a [`bundle::SdkBundle`] with stable file markers; the pipeline is byte-identical
//! across runs and never panics (RUST-04). [`write_to_dir`] (Task 3) materializes the same framing.

mod bundle;
mod emit;
