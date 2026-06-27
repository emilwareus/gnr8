//! Runtime resource discovery for installed gnr8 archives.
//!
//! The in-repo development path can resolve sidecars with `CARGO_MANIFEST_DIR`, but a released
//! `gnr8` binary runs from an install/archive layout. Release archives place the source resources under
//! `share/gnr8/`, and the host passes that location to the `.gnr8` child via `GNR8_RESOURCE_DIR`.

use std::path::{Path, PathBuf};

/// Environment variable used by the host to tell the `.gnr8` child where release resources live.
pub const GNR8_RESOURCE_DIR_ENV: &str = "GNR8_RESOURCE_DIR";

/// Locate the installed gnr8 resource root.
///
/// The expected root contains `goextract/`, `pyextract/`, `tsextract/`, and `crates/gnr8-core/`.
/// Resolution order:
/// 1. `$GNR8_RESOURCE_DIR` (used by the host when spawning the `.gnr8` child).
/// 2. Paths adjacent to the current executable, matching the release archive layout.
#[must_use]
pub fn resource_dir() -> Option<PathBuf> {
    if let Ok(value) = std::env::var(GNR8_RESOURCE_DIR_ENV) {
        let path = PathBuf::from(value);
        if looks_like_resource_dir(&path) {
            return Some(path);
        }
    }

    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    [
        parent.join("../share/gnr8"),
        parent.join("share/gnr8"),
        parent.join("../Resources/gnr8"),
    ]
    .into_iter()
    .find(|candidate| looks_like_resource_dir(candidate))
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
