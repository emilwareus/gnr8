//! Runtime resource resolution for gnr8 sidecars and installed source.
//!
//! The in-repo development path can resolve sidecars with `CARGO_MANIFEST_DIR`, but a released
//! `gnr8` binary runs from an install/archive layout. Release archives place the source resources under
//! `share/gnr8/`, and the host passes that location to the `.gnr8` child via `GNR8_RESOURCE_DIR`.
//!
//! Exactly one root is *selected* per process, then validated. Nothing is probed: when the selected
//! root is incomplete the call fails with the path that was selected and why, rather than silently
//! continuing to a second location. A stale install can therefore never supply sidecars to a binary
//! that did not ship it.
//!
//! The selection:
//! - `$GNR8_RESOURCE_DIR` when set — the host's explicit declaration to the `.gnr8` child, and the
//!   user's escape hatch. Always wins, in every build kind.
//! - otherwise, in debug builds, the compile-time repository root (`CARGO_MANIFEST_DIR/../..`).
//! - otherwise, `../share/gnr8` relative to the **canonicalized** executable, so invoking the
//!   installer's `~/.local/bin/gnr8` symlink resolves against the real `~/.local/gnr8/bin/gnr8`.

use std::path::{Path, PathBuf};

/// Environment variable used by the host to tell the `.gnr8` child where release resources live.
pub const GNR8_RESOURCE_DIR_ENV: &str = "GNR8_RESOURCE_DIR";

/// Resolve the one resource root selected for this process.
///
/// The expected root contains `goextract/`, `pyextract/`, `tsextract/`, and `crates/gnr8-core/`.
///
/// `$GNR8_RESOURCE_DIR` selects the root when set — this is how the host hands its own resolved
/// root to the `.gnr8` child, so the child agrees with the host by construction instead of
/// re-deriving it. With the variable unset, debug builds select the compile-time repository root and
/// release builds select `../share/gnr8` beside the canonicalized executable.
///
/// The selected root is validated and a failure is reported against that one path; no alternate
/// location is probed.
///
/// # Errors
///
/// Returns [`crate::CoreError::Io`] when the selected root cannot be derived or does not contain the
/// complete resource set.
pub fn resource_dir() -> Result<PathBuf, crate::CoreError> {
    let selected = match std::env::var(GNR8_RESOURCE_DIR_ENV) {
        Ok(value) => PathBuf::from(value),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(crate::CoreError::Io {
                message: format!("{GNR8_RESOURCE_DIR_ENV} is not valid Unicode"),
            });
        }
        Err(std::env::VarError::NotPresent) => default_resource_dir()?,
    };
    validate_resource_dir(normalize(&selected))
}

/// The root selected when `$GNR8_RESOURCE_DIR` is unset.
///
/// An in-tree build always knows its own repository root, so this cannot fail — but it shares the
/// release variant's signature so [`resource_dir`] has one call site rather than two cfg'd ones.
#[cfg(debug_assertions)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "signature parity with the release variant of this function"
)]
fn default_resource_dir() -> Result<PathBuf, crate::CoreError> {
    Ok(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")))
}

/// The root selected when `$GNR8_RESOURCE_DIR` is unset.
///
/// Canonicalizing the executable is what makes the packaged install work through its PATH symlink:
/// `~/.local/bin/gnr8 -> ~/.local/gnr8/bin/gnr8` must resolve `share/gnr8` beside the *real* binary,
/// not beside the link.
#[cfg(not(debug_assertions))]
fn default_resource_dir() -> Result<PathBuf, crate::CoreError> {
    let exe = std::env::current_exe().map_err(|source| crate::CoreError::Io {
        message: format!("failed to resolve the gnr8 executable for resource lookup: {source}"),
    })?;
    let real = normalize(&exe);
    let parent = real.parent().ok_or_else(|| crate::CoreError::Io {
        message: format!(
            "gnr8 executable has no parent directory: {}",
            real.display()
        ),
    })?;
    Ok(parent.join("../share/gnr8"))
}

/// Resolve `.`/`..`/symlink components when the path exists, leaving it untouched when it does not
/// (so the diagnostic reports the path as selected rather than an empty string).
fn normalize(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{looks_like_resource_dir, normalize, validate_resource_dir};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn unique_temp(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        std::env::temp_dir().join(format!(
            "gnr8-resource-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn write_complete_resources(root: &Path) {
        for (rel, contents) in [
            ("goextract/go.mod", "module example\n"),
            ("pyextract/__main__.py", "print('ok')\n"),
            ("tsextract/index.js", "export {}\n"),
            ("crates/gnr8-core/Cargo.toml", "[package]\nname=\"gnr8\"\n"),
        ] {
            let path = root.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, contents).unwrap();
        }
    }

    #[test]
    fn missing_declared_resource_root_is_an_explicit_error() {
        let missing = unique_temp("missing");
        let error = validate_resource_dir(missing).unwrap_err();
        assert!(
            error.to_string().contains("resource directory"),
            "unexpected diagnostic: {error}"
        );
    }

    #[test]
    fn complete_resource_layout_is_accepted() {
        let root = unique_temp("complete");
        write_complete_resources(&root);
        assert!(looks_like_resource_dir(&root));
        assert_eq!(
            validate_resource_dir(normalize(&root)).unwrap(),
            fs::canonicalize(&root).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    /// An incomplete root is reported against itself — never silently replaced by another location.
    ///
    /// This is the invariant that keeps a stale `~/.local/gnr8` from feeding sidecars to a binary
    /// that did not ship it: there is no second candidate to fall through to.
    #[test]
    fn an_incomplete_root_names_itself_and_is_not_replaced() {
        let partial = unique_temp("partial");
        fs::create_dir_all(partial.join("goextract")).unwrap();
        fs::write(partial.join("goextract/go.mod"), "module example\n").unwrap();

        let error = validate_resource_dir(normalize(&partial)).unwrap_err();
        let text = error.to_string();
        assert!(
            text.contains(&partial.display().to_string())
                || text.contains(&fs::canonicalize(&partial).unwrap().display().to_string()),
            "diagnostic must name the selected root: {text}"
        );
        let _ = fs::remove_dir_all(partial);
    }

    /// The installer exposes `~/.local/bin/gnr8` as a symlink to `~/.local/gnr8/bin/gnr8`.
    ///
    /// `share/gnr8` must resolve beside the *real* binary, so the release selection canonicalizes
    /// the executable before taking its parent.
    #[cfg(unix)]
    #[test]
    fn symlink_invocation_resolves_share_relative_to_real_executable() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let install = unique_temp("install");
        let bin = install.join("bin");
        let share = install.join("share/gnr8");
        let link_dir = unique_temp("bin-link");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&link_dir).unwrap();
        write_complete_resources(&share);

        let real_exe = bin.join("gnr8");
        fs::write(&real_exe, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&real_exe, fs::Permissions::from_mode(0o755)).unwrap();
        let link = link_dir.join("gnr8");
        symlink(&real_exe, &link).unwrap();

        // The release selection: canonicalize the invoked path, then take `../share/gnr8`.
        let candidate = normalize(&link).parent().unwrap().join("../share/gnr8");
        let resolved = validate_resource_dir(normalize(&candidate)).unwrap();
        assert_eq!(resolved, fs::canonicalize(&share).unwrap());

        // Without canonicalization the link's own directory has no sibling `share/`, which is the
        // failure this selection exists to prevent.
        let naive = link.parent().unwrap().join("../share/gnr8");
        assert!(validate_resource_dir(normalize(&naive)).is_err());

        let _ = fs::remove_dir_all(install);
        let _ = fs::remove_dir_all(link_dir);
    }
}
