//! Versioned, deterministic serialization of the projected API graph.
//!
//! [`GRAPH_ARTIFACT_PATH`] is an always-on generated artifact. A committed copy is the sole source
//! of historical graph facts for `gnr8 changes`; generation never needs to execute an older
//! revision's pipeline.

use crate::graph::ApiGraph;

/// Project-relative path of the generated graph artifact.
pub const GRAPH_ARTIFACT_PATH: &str = "generated/gnr8.graph.json";

/// Current on-disk graph artifact schema.
pub const GRAPH_ARTIFACT_SCHEMA_VERSION: u32 = 1;

/// Stable on-disk envelope for one projected [`ApiGraph`].
///
/// The envelope version belongs to the artifact format rather than the worker protocol or the graph
/// itself. This lets readers reject a graph written by a different artifact schema explicitly.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GraphArtifact {
    /// Version of this envelope and its embedded graph representation.
    pub schema_version: u32,
    /// The post-transform, generation-projected graph every target consumed.
    pub graph: ApiGraph,
}

impl GraphArtifact {
    /// Wrap a projected graph in the current artifact schema.
    #[must_use]
    pub fn new(graph: ApiGraph) -> Self {
        Self {
            schema_version: GRAPH_ARTIFACT_SCHEMA_VERSION,
            graph,
        }
    }

    /// Serialize this artifact as deterministic, pretty JSON with one trailing newline.
    ///
    /// # Errors
    ///
    /// Returns a typed graph-artifact error if the graph cannot be serialized.
    pub fn to_json(&self) -> Result<String, crate::CoreError> {
        let mut text =
            serde_json::to_string_pretty(self).map_err(|err| crate::CoreError::GraphArtifact {
                message: format!("failed to serialize {GRAPH_ARTIFACT_PATH}: {err}"),
            })?;
        text.push('\n');
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{GraphArtifact, GRAPH_ARTIFACT_SCHEMA_VERSION};
    use crate::graph::ApiGraph;

    #[test]
    fn graph_artifact_is_deterministic_and_round_trips() {
        let graph = ApiGraph {
            title: "Deterministic API".to_string(),
            ..ApiGraph::default()
        };
        let artifact = GraphArtifact::new(graph.clone());

        let first = artifact.to_json().expect("serialize graph artifact");
        let second = artifact.to_json().expect("serialize graph artifact again");
        assert_eq!(first, second);
        assert!(first.ends_with('\n'));

        let decoded: GraphArtifact =
            serde_json::from_str(&first).expect("deserialize graph artifact");
        assert_eq!(decoded.schema_version, GRAPH_ARTIFACT_SCHEMA_VERSION);
        assert_eq!(decoded.graph, graph);
    }
}
