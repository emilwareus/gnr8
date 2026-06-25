//! The multi-file SDK bundle and its deterministic file-marker framing (D-06).
//!
//! `sdk::generate` returns a single `String` so the `snapshot_sdk` contract test can lock the whole SDK
//! in one reviewable artifact. To keep that String unambiguous and round-trippable, each generated Go
//! file is framed by a stable, greppable marker line:
//!
//! ```text
//! // ==== gnr8:file client.go ====
//! <gofmt'd contents of client.go>
//! // ==== gnr8:file errors.go ====
//! <gofmt'd contents of errors.go>
//! ...
//! ```
//!
//! The marker never appears in `gofmt`'d Go (RESEARCH Code Examples), so [`parse`] can split the bundle
//! back into `(name, contents)` pairs — the SAME framing `write_to_dir` uses to materialize files
//! (single source of truth). File order is FIXED + sorted (`client.go`, `errors.go`, `operations.go`,
//! then `models.go`), and `to_string` is byte-identical across runs (determinism, T-03-02-03).

/// One generated Go file: its on-disk name (e.g. `client.go`) and its `gofmt`'d contents.
#[derive(Debug, Clone)]
pub(crate) struct SdkFile {
    /// The file name written to disk and embedded in the frame marker (e.g. `"models.go"`).
    pub(crate) name: String,
    /// The canonical (`gofmt`'d) Go source.
    pub(crate) contents: String,
}

/// An ordered set of generated Go files forming the SDK package.
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
/// Implemented as [`std::fmt::Display`] so the conventional `bundle.to_string()` (the D-06 contract the
/// snapshot locks) comes from the blanket `ToString` impl. Each file is rendered as its marker line, a
/// newline, then its contents (which already end in a trailing newline from `gofmt`); the output is
/// byte-identical for the same input across runs.
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
/// The inverse of [`SdkBundle::to_string`]; `write_to_dir` and the round-trip test share this single
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

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{parse, SdkBundle, SdkFile};

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
        // The marker prefix must not appear inside any framed Go content, or parse would mis-split.
        let bundle = sample_bundle();
        for file in &bundle.files {
            assert!(
                !file.contents.contains("// ==== gnr8:file"),
                "marker must not appear in gofmt'd Go"
            );
        }
    }
}
