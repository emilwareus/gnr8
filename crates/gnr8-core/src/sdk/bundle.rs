//! The multi-file SDK bundle and its deterministic file-marker framing (D-06).
//!
//! Each per-language `generate` returns a single `String` so the whole SDK is locked in one reviewable
//! artifact. To keep that String unambiguous and round-trippable, each generated file is framed by a
//! stable, greppable marker line:
//!
//! ```text
//! // ==== gnr8:file client.go ====
//! <contents of client.go>
//! // ==== gnr8:file models.go ====
//! <contents of models.go>
//! ...
//! ```
//!
//! The marker is a Go-style `//` comment line; it never appears inside any emitted source and [`parse`]
//! strips it before any file is written, so the framing is shared byte-identically across the Go, Python,
//! and TypeScript emitters (single source of truth). [`parse`] splits the bundle back into
//! `(name, contents)` pairs — the SAME framing [`write_to_dir`] uses to materialize files. File order is
//! FIXED + sorted by each emitter's push order, and `to_string` is byte-identical across runs
//! (determinism).

/// One generated SDK file: its on-disk name (e.g. `client.go`) and its emitted contents.
#[derive(Debug, Clone)]
pub(crate) struct SdkFile {
    /// The file name written to disk and embedded in the frame marker (e.g. `"models.go"`).
    pub(crate) name: String,
    /// The emitted source.
    pub(crate) contents: String,
}

/// An ordered set of generated files forming the SDK package.
#[derive(Debug, Clone, Default)]
pub(crate) struct SdkBundle {
    /// Files in their fixed, sorted emission order (see module docs).
    pub(crate) files: Vec<SdkFile>,
}

/// The frame marker prefix; `<name>` and the trailing ` ====` complete the line.
const MARKER_PREFIX: &str = "// ==== gnr8:file ";
/// The frame marker suffix.
const MARKER_SUFFIX: &str = " ====";

/// Build the full marker line for `name`.
fn marker_for(name: &str) -> String {
    format!("{MARKER_PREFIX}{name}{MARKER_SUFFIX}")
}

/// Serialize the bundle into a single deterministic String with stable per-file frame markers.
///
/// Implemented as [`std::fmt::Display`] so the conventional `bundle.to_string()` comes from the blanket
/// `ToString` impl. Each file is rendered as its marker line, a newline, then its contents (which
/// already end in a trailing newline from the emitters); the output is byte-identical for the same input
/// across runs.
impl std::fmt::Display for SdkBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for file in &self.files {
            writeln!(f, "{}", marker_for(&file.name))?;
            f.write_str(&file.contents)?;
            // Guarantee a separating newline even if a file's contents somehow lack a trailing one.
            if !file.contents.ends_with('\n') {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

/// Parse a bundle String back into `(name, contents)` pairs by splitting on the frame markers.
///
/// The inverse of [`SdkBundle::to_string`]; [`write_to_dir`] and the round-trip test share this single
/// framing definition. Any leading text before the first marker is ignored (there is none in practice).
/// Contents preserve the file's trailing newline.
pub(crate) fn parse(bundle: &str) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;

    for line in bundle.split_inclusive('\n') {
        if let Some(name) = parse_marker(line) {
            if let Some(pair) = current.take() {
                files.push(pair);
            }
            current = Some((name, String::new()));
        } else if let Some((_, contents)) = current.as_mut() {
            contents.push_str(line);
        }
    }
    if let Some(pair) = current.take() {
        files.push(pair);
    }
    files
}

/// If `line` is a frame marker, return the framed file name; otherwise `None`.
fn parse_marker(line: &str) -> Option<String> {
    let trimmed = line.trim_end_matches(['\n', '\r']);
    let rest = trimmed.strip_prefix(MARKER_PREFIX)?;
    let name = rest.strip_suffix(MARKER_SUFFIX)?;
    Some(name.to_string())
}

/// Reject a frame path that could traverse out of the target dir (defense-in-depth; the names are
/// program-generated). Nested relative paths are allowed so split layouts can write files such as
/// `models/book.ts`.
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] if `name` is empty, absolute, contains `..`, or uses Windows
/// separators.
pub(crate) fn safe_frame_name(name: &str) -> Result<(), crate::CoreError> {
    let path = std::path::Path::new(name);
    if name.is_empty()
        || name.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(crate::CoreError::SdkGen {
            message: format!("refusing to write SDK file with unsafe name {name:?}"),
        });
    }
    Ok(())
}

/// Materialize a generated SDK bundle String's framed files to `dir/<name>`.
///
/// Takes the public per-language `generate` output (the file-marker-framed bundle String) so an
/// out-of-crate integration test can call it directly. File names are program-controlled — they come
/// from the fixed per-language frame markers, never untrusted input — and are validated by
/// [`safe_frame_name`] before being joined onto the caller's program-controlled `dir`. The bundle is
/// split through the shared [`parse`] framing so the on-disk files match the bundle byte-for-byte. The
/// framing is language-agnostic, so this one definition serves the Go, Python, and TypeScript SDKs.
///
/// # Errors
///
/// Returns [`crate::CoreError::SdkGen`] if a frame name is empty, absolute, parent-traversing, or uses
/// platform-ambiguous separators (so no frame can escape `dir`) or if any file cannot be written.
pub fn write_to_dir(bundle: &str, dir: &std::path::Path) -> Result<(), crate::CoreError> {
    for (name, contents) in parse(bundle) {
        safe_frame_name(&name)?;
        let path = dir.join(&name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| crate::CoreError::SdkGen {
                message: format!(
                    "failed to create SDK output dir {}: {err}",
                    parent.display()
                ),
            })?;
        }
        std::fs::write(&path, contents).map_err(|err| crate::CoreError::SdkGen {
            message: format!("failed to write SDK file {}: {err}", path.display()),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{parse, safe_frame_name, SdkBundle, SdkFile};

    fn sample_bundle() -> SdkBundle {
        SdkBundle {
            files: vec![
                SdkFile {
                    name: "client.go".to_string(),
                    contents: "package sdk\n\nfunc NewClient() {}\n".to_string(),
                },
                SdkFile {
                    name: "errors.go".to_string(),
                    contents: "package sdk\n\ntype APIError struct{}\n".to_string(),
                },
                SdkFile {
                    name: "operations.go".to_string(),
                    contents: "package sdk\n\nfunc (c *Client) CreateGoal() {}\n".to_string(),
                },
                SdkFile {
                    name: "models.go".to_string(),
                    contents: "package sdk\n\ntype CreateGoalInput struct{}\n".to_string(),
                },
            ],
        }
    }

    #[test]
    fn to_string_frames_each_file_with_a_stable_marker_and_round_trips() {
        let bundle = sample_bundle();
        let text = bundle.to_string();

        // Each file is framed by its marker, in the fixed order.
        let order: Vec<_> = ["client.go", "errors.go", "operations.go", "models.go"]
            .iter()
            .map(|n| text.find(&format!("// ==== gnr8:file {n} ====")).unwrap())
            .collect();
        assert!(
            order.windows(2).all(|w| w[0] < w[1]),
            "markers must appear in fixed sorted order:\n{text}"
        );

        // Round-trip: parsing the bundle recovers the same (name, contents) pairs.
        let parsed = parse(&text);
        let expected: Vec<(String, String)> = bundle
            .files
            .iter()
            .map(|f| (f.name.clone(), f.contents.clone()))
            .collect();
        assert_eq!(parsed, expected, "framing must round-trip");
    }

    #[test]
    fn to_string_is_byte_identical_across_two_runs() {
        let bundle = sample_bundle();
        assert_eq!(
            bundle.to_string(),
            bundle.to_string(),
            "to_string must be deterministic"
        );
    }

    #[test]
    fn marker_never_collides_with_file_contents() {
        // The marker prefix must not appear inside any framed content, or parse would mis-split.
        let bundle = sample_bundle();
        for file in &bundle.files {
            assert!(
                !file.contents.contains("// ==== gnr8:file"),
                "marker must not appear in emitted source"
            );
        }
    }

    #[test]
    fn safe_frame_name_allows_nested_relative_paths_for_split_layouts() {
        for name in [
            "models/book.ts",
            "models/__init__.py",
            "nested/model_book.go",
        ] {
            safe_frame_name(name).unwrap();
        }
    }

    #[test]
    fn safe_frame_name_rejects_paths_that_can_escape_or_are_platform_ambiguous() {
        for name in [
            "",
            "../escape.ts",
            "models/../../escape.py",
            "/tmp/escape.go",
            "models\\book.ts",
        ] {
            assert!(
                safe_frame_name(name).is_err(),
                "unsafe frame name should be rejected: {name}"
            );
        }
    }
}
