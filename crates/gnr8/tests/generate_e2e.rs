//! Host→child→write end-to-end integration test for `gnr8 generate` (the code-as-config boundary).
//!
//! This is the ONE test that exercises the WHOLE real path: it scaffolds a `.gnr8/` generation crate
//! (`gnr8 init`), then runs the installed `gnr8` host binary, which compiles + runs that crate as a
//! child process (`cargo run --manifest-path`), receives the artifact bundle, and writes the files
//! (ownership manifest, no-op skip). It asserts the OpenAPI doc + Go SDK land on disk and that a SECOND
//! `gnr8 generate` is a true no-op (every output unchanged). The pure write machinery + the truth table
//! are covered fast/synthetically in `gnr8-core/tests/lifecycle.rs`; THIS proves the orchestration.
//!
//! Cost + environment: it cargo-compiles the child crate (which builds `gnr8-core` once in the child's
//! own target dir) and runs the Go toolchain (the `GoGin` source shells out to goextract; the `GoSdk`
//! target pipes Go through gofmt). It SKIPS gracefully (early return) when Go or cargo is unavailable,
//! mirroring the Go-dependent contract tests. The staging dir lives under `CARGO_TARGET_TMPDIR`
//! (`<repo>/target/tmp`), which is INSIDE the gnr8 repo, so `gnr8 init` detects `crates/gnr8-core` and
//! scaffolds a working `path` dependency to the public `gnr8` package source.

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4); scope the allow to this
// test target so the workspace-wide RUST-04 deny stays intact for production code. `doc_markdown` is
// allowed for the acronym-dense prose doc comments (OpenAPI, SDK, ...) — mirrors the other test files.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::doc_markdown
)]

use std::path::Path;
use std::process::Command;

/// The installed `gnr8` host binary cargo built for this integration test.
const GNR8_BIN: &str = env!("CARGO_BIN_EXE_gnr8");

/// Whether the Go + gofmt + cargo toolchains are all available so the e2e skips gracefully otherwise.
fn toolchains_available() -> bool {
    let probe = |bin: &str, arg: &str| {
        Command::new(bin)
            .arg(arg)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    };
    probe("go", "version") && probe("gofmt", "-h") && probe("cargo", "--version")
}

/// A minimal, self-contained Gin module staged under a unique temp dir, so the e2e controls exactly
/// what is generated (one route, one request model, one response model) without depending on the
/// shared fixture's shape. `<dir>/main.go` + `<dir>/go.mod` form a buildable module.
fn write_min_gin_module(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create module dir");
    std::fs::write(dir.join("go.mod"), "module example.com/e2e\n\ngo 1.21\n")
        .expect("write go.mod");
    // A tiny net/http-free Gin-shaped handler set the analyzer recognizes: a POST that binds a request
    // body and a GET that returns a typed response. No external imports beyond the stdlib shapes the
    // goextract helper recognizes structurally (it does not compile the module, it parses + typechecks).
    std::fs::write(
        dir.join("main.go"),
        r#"package main

// CreateThingRequest is the POST body.
type CreateThingRequest struct {
	Name string `json:"name" binding:"required"`
}

// ThingResponse is the GET response.
type ThingResponse struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type ginContext struct{}

func (c *ginContext) ShouldBindJSON(any) error { return nil }
func (c *ginContext) JSON(int, any)            {}
func (c *ginContext) Param(string) string      { return "" }

type ginEngine struct{}

func (e *ginEngine) POST(string, func(*ginContext)) {}
func (e *ginEngine) GET(string, func(*ginContext))  {}

func createThing(c *ginContext) {
	var req CreateThingRequest
	_ = c.ShouldBindJSON(&req)
	c.JSON(201, ThingResponse{})
}

func getThing(c *ginContext) {
	c.JSON(200, ThingResponse{})
}

func main() {
	r := &ginEngine{}
	r.POST("/things", createThing)
	r.GET("/things/:id", getThing)
}
"#,
    )
    .expect("write main.go");
}

/// Run `gnr8 <args...>` with `current_dir = root`, returning (success, stdout, stderr).
fn run_gnr8(root: &Path, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(GNR8_BIN)
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn the gnr8 host binary");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn generate_e2e_scaffolds_compiles_runs_and_is_idempotent() {
    if !toolchains_available() {
        eprintln!("skipping generate_e2e: go/gofmt/cargo toolchain unavailable");
        return;
    }

    // Stage under CARGO_TARGET_TMPDIR (<repo>/target/tmp) so `gnr8 init` finds crates/gnr8-core and
    // scaffolds a working path dep. Unique per run (PID + nanos) for hermeticity.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("gnr8-e2e-{}-{nanos}", std::process::id()));
    write_min_gin_module(&root);

    // 1. init scaffolds the mandatory .gnr8/ crate.
    let (ok, out, err) = run_gnr8(&root, &["init"]);
    assert!(
        ok,
        "gnr8 init must succeed.\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert!(
        root.join(".gnr8").join("Cargo.toml").is_file()
            && root.join(".gnr8").join("src").join("main.rs").is_file(),
        "init must scaffold .gnr8/Cargo.toml + src/main.rs"
    );

    // 2. generate: the host compiles + runs the child crate, then writes the outputs. The default
    //    scaffolded pipeline writes openapi.yaml + sdk/ at the project root. This may take tens of
    //    seconds on the cold child build.
    let (ok, out, err) = run_gnr8(&root, &["generate"]);
    assert!(
        ok,
        "gnr8 generate must succeed (host→child→write).\nstdout:\n{out}\nstderr:\n{err}"
    );

    // The OpenAPI doc + the Go SDK files must have landed on disk.
    let openapi = root.join("openapi.yaml");
    assert!(openapi.is_file(), "generate must write openapi.yaml");
    let openapi_text = std::fs::read_to_string(&openapi).expect("read openapi.yaml");
    assert!(
        openapi_text.contains("openapi: 3.1.0") && openapi_text.contains("ThingResponse"),
        "the generated OpenAPI must carry the analyzed schema:\n{openapi_text}"
    );
    for name in ["client.go", "errors.go", "operations.go", "models.go"] {
        let path = root.join("sdk").join(name);
        assert!(path.is_file(), "generate must write sdk/{name}");
        // The default pipeline includes Header::generated(), so every .go file is banner-stamped.
        let text = std::fs::read_to_string(&path).expect("read sdk file");
        assert!(
            text.starts_with("// Code generated by gnr8. DO NOT EDIT.\n"),
            "sdk/{name} must carry the generated header:\n{text}"
        );
    }

    // 3. A SECOND generate over unchanged source is a true no-op: 0 written, all unchanged. The child's
    //    loop-safety excludes the just-written sdk/*.go from re-analysis, so the artifacts are identical.
    let (ok, out, err) = run_gnr8(&root, &["generate"]);
    assert!(
        ok,
        "second generate must succeed.\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert!(
        out.contains("0 written"),
        "a second generate over unchanged source must write nothing (no-op):\n{out}"
    );

    // 4. `gnr8 check` reports up-to-date (exit 0) after the no-op.
    let (ok, out, _err) = run_gnr8(&root, &["check"]);
    assert!(
        ok && out.contains("up to date"),
        "gnr8 check must report up-to-date after a no-op generate:\n{out}"
    );

    // 5. A fresh CI checkout has committed generated artifacts but no local .gnr8/cache/manifest.json.
    //    `gnr8 check` must still pass when those artifacts are byte-identical to a fresh generation.
    std::fs::remove_dir_all(root.join(".gnr8").join("cache")).expect("remove .gnr8/cache");
    let (ok, out, err) = run_gnr8(&root, &["check"]);
    assert!(
        ok && out.contains("up to date"),
        "gnr8 check must pass in a fresh checkout without local cache when outputs match.\nstdout:\n{out}\nstderr:\n{err}"
    );

    // 6. If source changes after generation, `gnr8 check` must fail before SDKs are regenerated.
    let main_go = root.join("main.go");
    let source = std::fs::read_to_string(&main_go).expect("read main.go");
    let changed_source = source.replace(
        "type ThingResponse struct {\n\tID   string `json:\"id\"`\n\tName string `json:\"name\"`\n}",
        "type ThingResponse struct {\n\tID     string `json:\"id\"`\n\tName   string `json:\"name\"`\n\tStatus string `json:\"status\"`\n}",
    );
    assert_ne!(
        source, changed_source,
        "source fixture replacement must match"
    );
    std::fs::write(&main_go, changed_source).expect("change source");
    let (ok, out, err) = run_gnr8(&root, &["check"]);
    assert!(
        !ok && out.contains("not up to date"),
        "gnr8 check must fail when source changes require regenerated artifacts.\nstdout:\n{out}\nstderr:\n{err}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
