//! SDK generation profiles.
//!
//! Profiles are target-surface policy, not source facts. gnr8 currently exposes one owned SDK
//! surface; users customize it through typed target options and ordinary `.gnr8` code.

/// SDK generation profile shared by built-in targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkProfile;

impl SdkProfile {
    /// Preserve the historical minimal gnr8 SDK surface.
    #[must_use]
    pub fn minimal() -> Self {
        Self
    }

    /// Stable machine-readable profile name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        "minimal"
    }
}

impl Default for SdkProfile {
    fn default() -> Self {
        Self::minimal()
    }
}
