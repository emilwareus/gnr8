//! The internal API graph — re-exported from the thin `gnr8` SDK, plus the generation-time
//! algorithms that operate on it.
//!
//! The graph's *node types* are the one stable IR both sides of the host/worker boundary speak, so
//! they live in the published SDK crate (`gnr8::graph`) where a user's own `Transform` can read and
//! mutate them. What lives here is what only the host needs: the direction analysis and the
//! generation projection, which decide how a schema used in both request and response position
//! becomes two public models.
//!
//! Re-exporting rather than redefining keeps `crate::graph::ApiGraph` valid throughout the engine
//! and guarantees there is exactly one definition of every node type (CLAUDE.md rule 3).

pub(crate) mod direction;
pub(crate) mod projection;

pub use gnr8::graph::*;
