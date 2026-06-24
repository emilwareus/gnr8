//! Ownership manifest stub (RED) — real implementation lands in the GREEN step of Task 1.

/// Stub digest — returns an empty string so the manifest tests fail (RED).
#[must_use]
pub fn blake3_hex(_bytes: &[u8]) -> String {
    String::new()
}

/// Stub manifest — no fields recorded so the round-trip/prune tests fail (RED).
#[derive(Debug, Default)]
pub struct Manifest {
    /// Recorded files (always empty in the stub).
    pub files: Vec<()>,
}

impl Manifest {
    /// Stub: never records anything (RED).
    pub fn record(&mut self, _path: &str, _hash: &str, _source: &str) {}

    /// Stub: never finds a hash (RED).
    #[must_use]
    pub fn recorded_hash(&self, _path: &str) -> Option<&str> {
        None
    }

    /// Stub: never prunes (RED).
    pub fn prune_to(&mut self, _current_paths: &[String]) {}

    /// Stub: returns a stub error (RED).
    ///
    /// # Errors
    /// Always returns [`crate::CoreError::Manifest`] in the stub.
    pub fn save(&self, _gnr8_dir: &std::path::Path) -> Result<(), crate::CoreError> {
        Err(crate::CoreError::Manifest {
            message: "manifest stub".to_string(),
        })
    }
}

/// Stub loader — returns the empty default (RED for round-trip assertions).
///
/// # Errors
/// Never errors in the stub.
pub fn load(_gnr8_dir: &std::path::Path) -> Result<Manifest, crate::CoreError> {
    Ok(Manifest::default())
}
