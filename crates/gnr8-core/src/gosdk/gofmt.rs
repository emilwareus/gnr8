//! `gofmt` subprocess driver — normalize generated Go source to canonical formatting.
//!
//! Each emitted Go file (see [`super::emit`]) is normalized by the real `gofmt` binary so indentation,
//! import grouping, and alignment are canonical and byte-stable (D-05, RESEARCH Pattern 3) — Rust never
//! hand-aligns Go. The Go toolchain is already a hard project dependency, so `gofmt` is free.
//!
//! Security (threat T-03-02-SC / T-03-02-01): `gofmt` is spawned directly, never through a shell. The
//! single-file path feeds program-generated source on stdin; the multi-file path validates generated
//! relative file names, writes a temporary tree, and passes discrete path arguments to `gofmt -w`.
//!
//! No prod `unwrap`/`expect`/`panic` (RUST-04): a spawn failure (missing toolchain) →
//! [`CoreError::GoToolchainMissing`]; a non-zero exit (invalid Go) → [`CoreError::GoFmt`] carrying
//! stderr; `child.stdin.take()` is handled with a `let Some(..) else { return Err(..) }` — there is no
//! `.expect("piped")` (RESEARCH Pattern 3 caveat).

use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::sdk::bundle::{safe_frame_name, SdkFile};
use crate::CoreError;

/// Pipe `src` through the `gofmt` binary and return the canonically-formatted Go source.
///
/// # Errors
///
/// - [`CoreError::GoToolchainMissing`] if `gofmt` cannot be spawned (binary absent / not on `PATH`).
/// - [`CoreError::GoFmt`] if `gofmt` exits non-zero (e.g. syntactically-invalid Go), carrying the exit
///   code + captured stderr — never a panic.
pub(crate) fn gofmt(src: &str) -> Result<String, CoreError> {
    gofmt_with("gofmt", src)
}

/// Format generated Go files with one batched `gofmt` process.
///
/// Split SDK layouts can produce hundreds of small Go files. Running `gofmt` once per file pays process
/// startup latency hundreds of times, so multi-file generation writes a short-lived temp tree, runs one
/// batched `gofmt -w`, then reads the files back in the same deterministic order as the input vector.
pub(crate) fn gofmt_files(files: Vec<SdkFile>) -> Result<Vec<SdkFile>, CoreError> {
    if files.len() <= 1 {
        let mut out = Vec::with_capacity(files.len());
        for file in files {
            out.push(SdkFile {
                name: file.name,
                contents: gofmt(&file.contents)?,
            });
        }
        return Ok(out);
    }

    gofmt_files_with("gofmt", files)
}

fn gofmt_files_with(bin: &str, files: Vec<SdkFile>) -> Result<Vec<SdkFile>, CoreError> {
    let root = create_temp_root()?;
    let result = gofmt_files_in_temp(bin, &root, files);
    let _ = fs::remove_dir_all(&root);
    result
}

fn gofmt_files_in_temp(
    bin: &str,
    root: &Path,
    files: Vec<SdkFile>,
) -> Result<Vec<SdkFile>, CoreError> {
    let mut paths = Vec::with_capacity(files.len());
    for file in &files {
        safe_frame_name(&file.name)?;
        let path = root.join(&file.name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| CoreError::Io {
                message: format!(
                    "failed to create gofmt temp dir {}: {err}",
                    parent.display()
                ),
            })?;
        }
        fs::write(&path, file.contents.as_bytes()).map_err(|err| CoreError::Io {
            message: format!("failed to write gofmt temp file {}: {err}", path.display()),
        })?;
        paths.push(path);
    }

    let mut cmd = Command::new(bin);
    cmd.arg("-w");
    for path in &paths {
        cmd.arg(path);
    }
    let output = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| CoreError::GoToolchainMissing { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(CoreError::GoFmt {
            code: output.status.code(),
            stderr,
        });
    }

    let mut out = Vec::with_capacity(files.len());
    for (file, path) in files.into_iter().zip(paths) {
        let contents = fs::read_to_string(&path).map_err(|err| CoreError::Io {
            message: format!("failed to read gofmt temp file {}: {err}", path.display()),
        })?;
        out.push(SdkFile {
            name: file.name,
            contents,
        });
    }
    Ok(out)
}

fn create_temp_root() -> Result<PathBuf, CoreError> {
    let base = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let prefix = format!("gnr8-gofmt-{}-{nanos}", std::process::id());

    for attempt in 0..100 {
        let candidate = base.join(format!("{prefix}-{attempt}"));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => {
                return Err(CoreError::Io {
                    message: format!(
                        "failed to create gofmt temp dir {}: {err}",
                        candidate.display()
                    ),
                });
            }
        }
    }

    Err(CoreError::Io {
        message: format!(
            "failed to create unique gofmt temp dir under {}",
            base.display()
        ),
    })
}

/// Inner driver parameterized on the binary name so tests can force a missing binary (toolchain-missing
/// path) without mutating the process `PATH`.
fn gofmt_with(bin: &str, src: &str) -> Result<String, CoreError> {
    // No args, no shell — the source is fed on stdin (T-03-02-SC).
    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| CoreError::GoToolchainMissing { source })?;

    // RUST-04 / RESEARCH Pattern 3 caveat: NO `.expect("piped")`. If stdin somehow did not open, fail
    // typed rather than panic.
    let Some(mut stdin) = child.stdin.take() else {
        return Err(CoreError::GoFmt {
            code: None,
            stderr: "failed to open gofmt stdin".to_string(),
        });
    };
    // Write the source, then drop stdin so gofmt sees EOF and can finish.
    if let Err(err) = stdin.write_all(src.as_bytes()) {
        return Err(CoreError::GoFmt {
            code: None,
            stderr: format!("failed to write to gofmt stdin: {err}"),
        });
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .map_err(|source| CoreError::GoToolchainMissing { source })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(CoreError::GoFmt {
            code: output.status.code(),
            stderr: format_gofmt_error(&stderr, src),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn format_gofmt_error(stderr: &str, src: &str) -> String {
    let Some(line) = first_gofmt_line(stderr) else {
        return stderr.to_string();
    };

    let lines: Vec<&str> = src.lines().collect();
    if lines.is_empty() {
        return stderr.to_string();
    }

    let start = line.saturating_sub(4).max(1);
    let end = (line + 4).min(lines.len());
    let mut out = stderr.to_string();
    out.push_str("\nsource excerpt:\n");
    for line_no in start..=end {
        let marker = if line_no == line { ">" } else { " " };
        let text = lines.get(line_no - 1).copied().unwrap_or("");
        let _ = writeln!(out, "{marker}{line_no:>5}: {text}");
    }
    out
}

fn first_gofmt_line(stderr: &str) -> Option<usize> {
    for line in stderr.lines() {
        let rest = line.strip_prefix("<standard input>:")?;
        let (line_no, _) = rest.split_once(':')?;
        if let Ok(parsed) = line_no.parse() {
            return Some(parsed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{gofmt, gofmt_files, gofmt_with};
    use crate::sdk::bundle::SdkFile;
    use crate::CoreError;

    /// Whether the `gofmt` binary is available, so toolchain-dependent tests skip gracefully (mirrors
    /// `tests/determinism.rs`) rather than failing for a missing dependency.
    fn gofmt_available() -> bool {
        std::process::Command::new("gofmt")
            .arg("-h")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    }

    #[test]
    fn formats_misindented_go_and_is_idempotent() {
        if !gofmt_available() {
            eprintln!("skipping gofmt formatting test: gofmt unavailable");
            return;
        }
        // Syntactically valid but mis-indented Go (multi-statement body so gofmt tab-indents it).
        let messy =
            "package x\nimport \"fmt\"\nfunc f(){\nfmt.Println(\"hi\")\nfmt.Println(\"bye\")\n}\n";
        let once = gofmt(messy).unwrap();
        // gofmt indents the body statements with a tab.
        assert!(
            once.contains("\tfmt.Println"),
            "expected tab-indented body:\n{once}"
        );
        // Idempotent: gofmt(gofmt(x)) == gofmt(x).
        let twice = gofmt(&once).unwrap();
        assert_eq!(once, twice, "gofmt must be idempotent");
    }

    #[test]
    fn formats_multiple_files_with_batched_path() {
        if !gofmt_available() {
            eprintln!("skipping gofmt batch formatting test: gofmt unavailable");
            return;
        }
        let files = vec![
            SdkFile {
                name: "a.go".to_string(),
                contents: "package x\nfunc a(){\n}\n".to_string(),
            },
            SdkFile {
                name: "nested/b.go".to_string(),
                contents: "package nested\nfunc b(){\n}\n".to_string(),
            },
        ];

        let formatted = gofmt_files(files).unwrap();

        assert_eq!(formatted[0].name, "a.go");
        assert_eq!(formatted[1].name, "nested/b.go");
        assert!(
            formatted[0].contents.contains("func a() {\n}"),
            "expected formatted function body:\n{}",
            formatted[0].contents
        );
        assert!(
            formatted[1].contents.contains("func b() {\n}"),
            "expected formatted nested function body:\n{}",
            formatted[1].contents
        );
    }

    #[test]
    fn invalid_go_maps_to_gofmt_error_not_panic() {
        if !gofmt_available() {
            eprintln!("skipping gofmt error test: gofmt unavailable");
            return;
        }
        // Syntactically invalid Go → gofmt exits non-zero.
        let broken = "package x\nfunc {{{ this is not go";
        let err = gofmt(broken).unwrap_err();
        match err {
            CoreError::GoFmt { stderr, .. } => {
                assert!(!stderr.is_empty(), "GoFmt error must carry stderr");
            }
            other => panic!("expected CoreError::GoFmt, got {other:?}"),
        }
    }

    #[test]
    fn missing_binary_maps_to_toolchain_missing() {
        let err = gofmt_with("gnr8-nonexistent-gofmt-binary-xyz", "package x\n").unwrap_err();
        assert!(
            matches!(err, CoreError::GoToolchainMissing { .. }),
            "expected GoToolchainMissing, got {err:?}"
        );
    }
}
