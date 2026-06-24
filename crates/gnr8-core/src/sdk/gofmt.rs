//! `gofmt` subprocess driver — normalize generated Go source to canonical formatting.
//!
//! Each emitted Go file (see [`super::emit`]) is piped through the real `gofmt` binary so indentation,
//! import grouping, and alignment are canonical and byte-stable (D-05, RESEARCH Pattern 3) — Rust never
//! hand-aligns Go. The Go toolchain is already a hard project dependency, so `gofmt` is free.
//!
//! Security (threat T-03-02-SC / T-03-02-01): `gofmt` is spawned with NO arguments and NO shell — the
//! program-generated source is fed on stdin, never interpolated into a command string. This mirrors the
//! verified discrete-arg subprocess pattern in `analyze::helper.rs`.
//!
//! No prod `unwrap`/`expect`/`panic` (RUST-04): a spawn failure (missing toolchain) →
//! [`CoreError::GoToolchainMissing`]; a non-zero exit (invalid Go) → [`CoreError::GoFmt`] carrying
//! stderr; `child.stdin.take()` is handled with a `let Some(..) else { return Err(..) }` — there is no
//! `.expect("piped")` (RESEARCH Pattern 3 caveat).

// `gofmt` is called by `sdk::generate` (wired in Task 3 of this plan). Until then it is exercised only
// by the unit tests below; allow dead_code so the `-D warnings` gate stays green this task. Removed once
// `generate` calls it.
#![allow(dead_code)]

use std::io::Write as _;
use std::process::{Command, Stdio};

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
        return Err(CoreError::GoFmt {
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    // Tests legitimately use unwrap/expect (rust-best-practices skill ch.4 + ch.5); scope the allow so
    // the workspace-wide RUST-04 deny stays intact for production code.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{gofmt, gofmt_with};
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
