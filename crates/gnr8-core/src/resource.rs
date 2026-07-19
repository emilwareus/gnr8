//! Runtime resource resolution for gnr8 sidecars and installed source.
//!
//! The in-repo development path can resolve sidecars with `CARGO_MANIFEST_DIR`, but a released
//! `gnr8` binary runs from an install/archive layout. Release archives place the source resources under
//! `share/gnr8/`, and the host passes that location to the `.gnr8` child via `GNR8_RESOURCE_DIR`.

use std::path::{Path, PathBuf};

/// Environment variable used by the host to tell the `.gnr8` child where release resources live.
pub const GNR8_RESOURCE_DIR_ENV: &str = "GNR8_RESOURCE_DIR";

/// Resolve the one resource root selected for this build.
///
/// The expected root contains `goextract/`, `pyextract/`, `tsextract/`, and `crates/gnr8-core/`.
/// Debug builds use the repository root selected at compile time. Release builds use an explicit
/// `$GNR8_RESOURCE_DIR` when supplied, otherwise the single archive location `../share/gnr8` relative
/// to the executable. The selected path is validated and failure is returned with an actionable
/// diagnostic; no alternate locations are probed.
///
/// # Errors
///
/// Returns [`crate::CoreError::Io`] when the selected root cannot be derived or does not contain the
/// complete resource set.
pub fn resource_dir() -> Result<PathBuf, crate::CoreError> {
    #[cfg(debug_assertions)]
    let selected = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));

    #[cfg(not(debug_assertions))]
    let selected = match std::env::var(GNR8_RESOURCE_DIR_ENV) {
        Ok(value) => PathBuf::from(value),
        Err(std::env::VarError::NotPresent) => {
            let exe = std::env::current_exe().map_err(|source| crate::CoreError::Io {
                message: format!(
                    "failed to resolve the gnr8 executable for resource lookup: {source}"
                ),
            })?;
            let parent = exe.parent().ok_or_else(|| crate::CoreError::Io {
                message: format!("gnr8 executable has no parent directory: {}", exe.display()),
            })?;
            parent.join("../share/gnr8")
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(crate::CoreError::Io {
                message: format!("{GNR8_RESOURCE_DIR_ENV} is not valid Unicode"),
            });
        }
    };

    validate_resource_dir(selected)
}

fn validate_resource_dir(path: PathBuf) -> Result<PathBuf, crate::CoreError> {
    if looks_like_resource_dir(&path) {
        return Ok(path);
    }
    Err(crate::CoreError::Io {
        message: format!(
            "gnr8 resource directory is missing or incomplete at {} — reinstall gnr8 or set {GNR8_RESOURCE_DIR_ENV} to the archive's share/gnr8 directory",
            path.display()
        ),
    })
}

fn looks_like_resource_dir(path: &Path) -> bool {
    path.join("goextract").join("go.mod").is_file()
        && path.join("pyextract").join("__main__.py").is_file()
        && path.join("tsextract").join("index.js").is_file()
        && path
            .join("crates")
            .join("gnr8-core")
            .join("Cargo.toml")
            .is_file()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::validate_resource_dir;

    #[test]
    fn missing_declared_resource_root_is_an_explicit_error() {
        let missing =
            std::env::temp_dir().join(format!("gnr8-missing-resources-{}", std::process::id()));
        let error = validate_resource_dir(missing).unwrap_err();
        assert!(
            error.to_string().contains("resource directory"),
            "unexpected diagnostic: {error}"
        );
    }
}
