//! Runtime resource resolution for gnr8 sidecars and installed source.
//!
//! The in-repo development path can resolve sidecars with `CARGO_MANIFEST_DIR`, but a released
//! `gnr8` binary runs from an install/archive layout. Release archives place the source resources under
//! `share/gnr8/`, and the host passes that location to the `.gnr8` child via `GNR8_RESOURCE_DIR`.
//!
//! Discovery order (first complete match wins):
//! 1. `$GNR8_RESOURCE_DIR` when set
//! 2. `../share/gnr8` relative to the canonicalized executable (symlink-safe)
//! 3. `../share/gnr8` relative to `gnr8` found on `$PATH`
//! 4. `$HOME/.local/gnr8/share/gnr8` (default installer layout)
//! 5. Compile-time repository root (`CARGO_MANIFEST_DIR/../..`) for in-tree development builds

use std::path::{Path, PathBuf};

/// Environment variable used by the host to tell the `.gnr8` child where release resources live.
pub const GNR8_RESOURCE_DIR_ENV: &str = "GNR8_RESOURCE_DIR";

/// Resolve the one resource root selected for this process.
///
/// The expected root contains `goextract/`, `pyextract/`, `tsextract/`, and `crates/gnr8-core/`.
///
/// Discovery order (first complete match wins):
/// 1. `$GNR8_RESOURCE_DIR` when set
/// 2. Compile-time repository root in debug builds (`cargo test` / `cargo run` from source)
/// 3. `../share/gnr8` relative to the canonicalized executable (symlink-safe)
/// 4. `../share/gnr8` relative to `gnr8` found on `$PATH`
/// 5. `$HOME/.local/gnr8/share/gnr8` (default installer layout)
///
/// # Errors
///
/// Returns [`crate::CoreError::Io`] when no candidate contains the complete resource set.
pub fn resource_dir() -> Result<PathBuf, crate::CoreError> {
    let mut attempted = Vec::new();

    match std::env::var(GNR8_RESOURCE_DIR_ENV) {
        Ok(value) => {
            let candidate = PathBuf::from(value);
            if let Some(valid) = try_resource_dir(&candidate) {
                return Ok(valid);
            }
            attempted.push(candidate);
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(crate::CoreError::Io {
                message: format!("{GNR8_RESOURCE_DIR_ENV} is not valid Unicode"),
            });
        }
    }

    // Prefer the in-tree checkout for debug builds so `cargo test` is not hijacked by a stale
    // packaged install under ~/.local/gnr8.
    #[cfg(debug_assertions)]
    {
        let compile_time = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        if let Some(valid) = try_resource_dir(&compile_time) {
            return Ok(valid);
        }
        attempted.push(compile_time);
    }

    if let Some(exe) = current_exe_canonical() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("../share/gnr8");
            if let Some(valid) = try_resource_dir(&candidate) {
                return Ok(valid);
            }
            attempted.push(candidate);
        }
    }

    if let Some(path_exe) = gnr8_on_path() {
        if let Some(parent) = path_exe.parent() {
            let candidate = parent.join("../share/gnr8");
            if let Some(valid) = try_resource_dir(&candidate) {
                return Ok(valid);
            }
            attempted.push(candidate);
        }
    }

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let candidate = home.join(".local/gnr8/share/gnr8");
        if let Some(valid) = try_resource_dir(&candidate) {
            return Ok(valid);
        }
        attempted.push(candidate);
    }

    #[cfg(not(debug_assertions))]
    {
        let compile_time = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        if let Some(valid) = try_resource_dir(&compile_time) {
            return Ok(valid);
        }
        attempted.push(compile_time);
    }

    let tried = attempted
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(crate::CoreError::Io {
        message: format!(
            "gnr8 resource directory is missing or incomplete (tried: {tried}) — reinstall gnr8 or set {GNR8_RESOURCE_DIR_ENV} to the archive's share/gnr8 directory"
        ),
    })
}

fn try_resource_dir(path: &Path) -> Option<PathBuf> {
    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if looks_like_resource_dir(&normalized) {
        Some(normalized)
    } else {
        None
    }
}

fn current_exe_canonical() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(std::fs::canonicalize(&exe).unwrap_or(exe))
}

fn gnr8_on_path() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("gnr8");
        if !candidate.is_file() {
            continue;
        }
        // Skip the current process when it is already on PATH — prefer a distinct install binary.
        if let Ok(current) = std::env::current_exe() {
            if let (Ok(a), Ok(b)) = (
                std::fs::canonicalize(&candidate),
                std::fs::canonicalize(&current),
            ) {
                if a == b {
                    continue;
                }
            }
        }
        return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
    }
    None
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

/// Validate that `path` looks like a complete gnr8 resource root.
///
/// # Errors
///
/// Returns [`crate::CoreError::Io`] when the directory is incomplete.
pub fn validate_resource_dir(path: PathBuf) -> Result<PathBuf, crate::CoreError> {
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

    use super::{looks_like_resource_dir, try_resource_dir, validate_resource_dir};
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
            try_resource_dir(&root).unwrap(),
            fs::canonicalize(&root).unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn symlink_invocation_resolves_share_relative_to_real_executable() {
        let install = unique_temp("install");
        let bin = install.join("bin");
        let share = install.join("share/gnr8");
        let link_dir = unique_temp("bin-link");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&link_dir).unwrap();
        write_complete_resources(&share);

        let real_exe = bin.join("gnr8");
        fs::write(&real_exe, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{symlink, PermissionsExt};
            fs::set_permissions(&real_exe, fs::Permissions::from_mode(0o755)).unwrap();
            let link = link_dir.join("gnr8");
            symlink(&real_exe, &link).unwrap();

            // Simulate the installer layout: discovery uses the real executable directory.
            let from_link = fs::canonicalize(&link).unwrap();
            let candidate = from_link.parent().unwrap().join("../share/gnr8");
            let resolved = try_resource_dir(&candidate).unwrap();
            assert_eq!(resolved, fs::canonicalize(&share).unwrap());
        }
        let _ = fs::remove_dir_all(install);
        let _ = fs::remove_dir_all(link_dir);
    }
}
