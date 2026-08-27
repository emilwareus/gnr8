//! The typed error a pipeline stage returns (RUST-04).
//!
//! A stage author writes `Result<T, gnr8::Error>`. The variants cover exactly the failure modes a
//! `Source`/`Transform`/`Target`/`PostProcess` can produce; everything else — toolchain spawning,
//! `OpenAPI` lowering, SDK emission, the filesystem writer — belongs to the host engine and never
//! reaches this crate.
//!
//! The enum is `#[non_exhaustive]`: matching it requires a wildcard arm, so a future variant is not a
//! breaking change for downstream Rust.

/// Errors produced by a pipeline stage or by the host/worker frame protocol.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A configuration fact the pipeline expressed is invalid or internally inconsistent.
    ///
    /// A source with no input directory, a target missing its output path, a rename that would
    /// collide — anything where the composition itself cannot proceed.
    #[error("config error: {message}")]
    Config {
        /// Human-readable failure detail (the offending value or an actionable hint).
        message: String,
    },

    /// A filesystem failure inside a stage.
    #[error("io error: {message}")]
    Io {
        /// Human-readable failure detail naming the offending operation/path.
        message: String,
    },

    /// A stage could not produce its output from the graph it was given.
    #[error("generation failed: {message}")]
    Generation {
        /// Human-readable failure detail.
        message: String,
    },

    /// An artifact producer attempted an invalid ownership transition.
    #[error("artifact ownership error [{code}] for '{path}' from {producer}: {message}")]
    ArtifactOwnership {
        /// Stable machine-enforceable identity such as `artifact.path_collision`.
        code: String,
        /// Project-relative artifact path involved in the transition.
        path: String,
        /// Pipeline stage that requested the transition.
        producer: String,
        /// Human-readable details, including the current owner when relevant.
        message: String,
    },

    /// The host and the worker do not agree on the frame protocol, its bounds, or its versions.
    #[error("protocol error: {message}")]
    Protocol {
        /// Actionable version, bound, or integrity mismatch detail.
        message: String,
    },
}

impl Error {
    /// Build a [`Error::Config`] from anything displayable.
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
        }
    }

    /// Build an [`Error::Io`] from anything displayable.
    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    /// Build an [`Error::Generation`] from anything displayable.
    pub fn generation(message: impl Into<String>) -> Self {
        Self::Generation {
            message: message.into(),
        }
    }

    /// Build an [`Error::Protocol`] from anything displayable.
    pub fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol {
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn display_names_the_failing_surface() {
        assert_eq!(
            Error::config("no source").to_string(),
            "config error: no source"
        );
        assert_eq!(
            Error::protocol("frame too large").to_string(),
            "protocol error: frame too large"
        );
        assert_eq!(
            Error::ArtifactOwnership {
                code: "artifact.path_collision".to_string(),
                path: "sdk/client.go".to_string(),
                producer: "target[1]:GoSdk".to_string(),
                message: "already owned".to_string(),
            }
            .to_string(),
            "artifact ownership error [artifact.path_collision] for 'sdk/client.go' from \
             target[1]:GoSdk: already owned"
        );
    }
}
