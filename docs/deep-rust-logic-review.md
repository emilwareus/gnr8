# Deep Rust Logic Review

Date: 2026-06-27
Branch reviewed: `deep-rust-code-review` at `2e0d3b38`, equal to `origin/main`
Scope: Rust core/CLI logic, generated SDK emitters, OpenAPI lowering, artifact lifecycle, and the extractor contracts that feed the Rust IR.

## Review Standard

This review used the repo's own lint posture plus the Rust best-practices skill as the bar:

- Library code should return typed errors and avoid production `unwrap`, `expect`, `panic`, and panic-equivalent macros.
- `gnr8-core` should stay on `thiserror`-style domain errors; `anyhow` belongs at the binary boundary only.
- Borrowing, deterministic ordering, and structured APIs should be preferred over clone-heavy or stringly logic.
- Cross-language contracts should be explicit: if a fact is accepted into the neutral IR, every target should either preserve it or reject it with a typed error.
- Caching must include every input that can influence generated artifacts.

## Verification Run

`make check` passed end-to-end during this review.

The passing run covered:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo test --all-features`
- Go fixture build/vet and `goextract` build/vet/tests
- `python3 -m unittest discover pyextract/tests`
- TypeScript extractor test suite
- `cargo build --release -p gnr8`
- Example generate/check workflows

Side effect from the verification run: five example `.gnr8/Cargo.lock` files were normalized from `gnr8-core` version `0.1.0` to the current crate version `0.0.10`.

## Fix Pass

Status: all 12 findings below were fixed in this workspace on 2026-06-27.

Post-fix verification passed:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features --locked -- -D warnings`
- `cargo test --all-features`
- `make check`

Fix summary:

| Finding | Status | Implementation |
| --- | --- | --- |
| 1. `StaticFiles` cache inputs | Fixed | Added target cache-input reporting and folded static source file content into the artifact cache key. |
| 2. Duplicate schema names | Fixed | Added shared SDK duplicate-name validation and OpenAPI component-name validation before emission. |
| 3. Alternate 2xx responses | Fixed | Replaced exact success-status checks with non-2xx rejection plus body-status-specific decoding in Go, Python, and TypeScript SDKs. |
| 4. Raw YAML map keys | Fixed | Quoted source-derived YAML mapping keys for components, properties, security schemes, and security requirements. |
| 5. Map-key contract | Fixed | Rejected non-string map keys in OpenAPI, Python SDK, and TypeScript SDK paths; preserved Python map value types. |
| 6. Transform validation | Fixed | Validated base paths, SDK base paths, and success-status overrides; success overrides now replace prior 2xx responses while preserving errors. |
| 7. Response-less operations | Fixed | Lowering and YAML fallback now emit an explicit `default` response instead of `responses: {}`. |
| 8. Verified no-op stamps | Fixed | File stamps now include content hashes, including generated output stamps used by the hot no-op path. |
| 9. `StaticFiles` path contract | Fixed | Rejected absolute, empty, and traversal source/output directories before reading or writing. |
| 10. TOML parsing | Fixed | Replaced ad hoc `Cargo.toml` line parsing with the `toml` crate. |
| 11. Go identifier `unreachable!` | Fixed | Replaced the production panic-equivalent with checked suffix increment and a typed `SdkGen` error. |
| 12. Stale phase docs | Fixed | Updated crate/error/CLI docs to describe the current architecture and generic SDK error boundary. |

Snapshot and generated-example updates were required because the OpenAPI and SDK outputs changed intentionally:

- Flask OpenAPI snapshots now show `default` responses for response-less routes.
- SDK snapshots and examples now show 2xx-range rejection, body-status-specific decoding, and explicit typed errors for undeclared typed-success 2xx fallthroughs.

## Positive Findings

- The workspace lint posture is strong: `unsafe_code = "forbid"`, clippy `all = deny`, and explicit denies for `unwrap_used`, `expect_used`, and `panic`.
- `gnr8-core` has a clean typed error boundary (`CoreError` in `crates/gnr8-core/src/error.rs`) while the CLI crate owns the `anyhow` boundary.
- The graph and lowering layers consistently prefer deterministic ordering (`Vec` sorted by stable keys, `BTreeSet` where de-duplication matters).
- Sidecar invocation is done with discrete subprocess arguments, not shell interpolation.
- Lifecycle planning centralizes path traversal checks through `safe_output_path`, and both plan-only and apply paths use it.
- Python and TypeScript SDK emitters already reject duplicate schema names, which is the right direction for target-specific symbol uniqueness.

## Findings

### 1. High: `StaticFiles` inputs are missing from the artifact cache key

Impact: changes to copied static files can be hidden by artifact cache hits. The pipeline can return cached artifacts without calling target generation, so `StaticFiles` source bytes are not re-read even though they influence emitted artifacts.

Evidence:

- `Pipeline::run_with_cache` returns cached artifacts before calling `target.generate` when `load_artifact_cache` succeeds (`crates/gnr8-core/src/sdk/mod.rs:399-424`).
- `artifact_cache_key` hashes only the frozen IR and config-surface fingerprint (`crates/gnr8-core/src/sdk/mod.rs:448-459`).
- `config_surface_fingerprint` only collects `.gnr8/src`, `.gnr8/Cargo.toml`, `.gnr8/Cargo.lock`, and the current executable (`crates/gnr8-core/src/sdk/mod.rs:462-475`).
- `StaticFiles::generate` reads project files at generation time (`crates/gnr8-core/src/sdk/builtins.rs:946-963`).
- The `Target` trait has output-anchor hooks, but no target input-fingerprint hook (`crates/gnr8-core/src/sdk/mod.rs:203-220`).

Recommendation: add a cache-input API to `Target`, or add a narrower built-in hook that lets `StaticFiles` contribute normalized source paths and content hashes to `artifact_cache_key`. Add a regression test that changes a static file while IR and `.gnr8` config stay fixed, then verifies regenerated artifacts include the changed text.

### 2. High: duplicate schema names are not rejected before OpenAPI and Go generation

Impact: two distinct schema ids with the same bare name can collide in OpenAPI components and Go type declarations. References are resolved through id-to-name mapping, so collisions can also make `$ref` targets ambiguous in generated OpenAPI.

Evidence:

- OpenAPI lowering maps every schema id to a bare component name without checking name uniqueness (`crates/gnr8-core/src/lower/mod.rs:100-108`).
- Component schemas are emitted as `(schema.name, object)` pairs without duplicate detection (`crates/gnr8-core/src/lower/mod.rs:305-317`).
- YAML components then render raw component keys in order (`crates/gnr8-core/src/lower/yaml.rs:174-177`).
- Go models iterate all schemas and emit `type {name} ...` for each schema without a preflight duplicate-name check (`crates/gnr8-core/src/gosdk/emit.rs:315-348`).
- Python and TypeScript have explicit duplicate-name regression tests, which means the cross-target behavior is inconsistent.

Recommendation: validate schema-name uniqueness once at the shared IR freeze/lowering boundary, or introduce a shared `ensure_unique_schema_names` helper that every target calls. The better long-term model is to keep symbol uniqueness as an IR invariant after transforms. Add OpenAPI and Go SDK tests matching the existing Python/TypeScript duplicate-name cases.

### 3. High: SDK clients reject valid alternate 2xx success responses

Impact: an operation with multiple valid 2xx responses is narrowed to one status in every SDK. A server returning a different successful status, such as `200` and `204` on the same operation, will be surfaced as an API error.

Evidence:

- `success_of` deliberately returns the first/lowest 2xx response (`crates/gnr8-core/src/sdk/emit_common.rs:270-283`).
- Go generation checks exact equality: `resp.StatusCode != {success_status}` (`crates/gnr8-core/src/gosdk/emit.rs:2500-2503`).
- Python generation checks exact equality: `_status != {success_status}` (`crates/gnr8-core/src/pysdk/emit.rs:1124-1130`).
- TypeScript generation checks exact equality: `res.status !== {success_status}` (`crates/gnr8-core/src/tssdk/emit.rs:812-821`).
- The Go comment says "Non-2xx", but the generated condition is "not this one 2xx".

Recommendation: separate "which response model do we decode" from "is this status successful". Generate `200 <= status < 300` checks, then decode only when the status is one of the modeled success statuses with a body. If that is too broad for the SDK contract, explicitly validate that each operation has at most one 2xx response and reject multi-success operations with a typed error.

### 4. Medium/High: OpenAPI YAML emits several map keys raw

Impact: generated YAML can become invalid or semantically wrong when schema names, property names, or security scheme ids contain YAML-sensitive characters. The code already quotes path keys because `{}` is dangerous, but does not apply the same discipline to other user/source-derived keys.

Evidence:

- Path keys are quoted intentionally (`crates/gnr8-core/src/lower/yaml.rs:63-69`).
- Component schema names are emitted raw (`crates/gnr8-core/src/lower/yaml.rs:174-177`).
- Security scheme ids are emitted raw (`crates/gnr8-core/src/lower/yaml.rs:181-186`).
- Object property names are emitted raw (`crates/gnr8-core/src/lower/yaml.rs:240-244`).

Recommendation: quote every YAML mapping key through the same scalar quoting function, not only paths and status codes. Add tests for property names like `x:y`, `a#b`, and `{id}` and for a security scheme id containing `:`.

### 5. Medium: map-key semantics are accepted into the IR but not consistently preserved

Impact: Go extraction can emit maps with non-string keys, but OpenAPI and TypeScript can only represent string-keyed objects, and Python currently drops even the value type. That means users can get generated contracts that silently misrepresent source types.

Evidence:

- `goextract` emits both key and value types for arbitrary Go maps (`goextract/internal/types/extract.go:208-215`).
- OpenAPI lowering ignores the key type and lowers every map to an object with `additionalProperties` for the value (`crates/gnr8-core/src/lower/mod.rs:445-450`).
- Python maps every map to `Dict[str, Any]`, losing both non-string key information and value typing (`crates/gnr8-core/src/pysdk/emit.rs:238-240`).
- TypeScript preserves the value type but always emits `Record<string, V>` (`crates/gnr8-core/src/tssdk/emit.rs:154-157`).
- Go SDK preserves key and value types as `map[K]V` (`crates/gnr8-core/src/gosdk/emit.rs:136-145`).

Recommendation: decide the neutral contract. The simplest robust rule is "maps exposed to OpenAPI/JSON SDKs must have string keys"; reject non-string map keys during graph construction or target lowering with a diagnostic tied to provenance. Also preserve Python value types as `Dict[str, V]` for supported string-key maps.

### 6. Medium: path and status transforms accept invalid values too late

Impact: code-as-config transforms can produce invalid OpenAPI paths or misleading SDK behavior without a typed configuration error at the source of the bad setting.

Evidence:

- `SetBasePath::new` stores any string (`crates/gnr8-core/src/sdk/builtins.rs:356-363`).
- `SetBasePath::apply` copies it directly into the graph (`crates/gnr8-core/src/sdk/builtins.rs:366-369`).
- `join_base` only trims and concatenates, so `SetBasePath::new("books")` produces `books/...` instead of an absolute OpenAPI path (`crates/gnr8-core/src/lower/mod.rs:524-533`).
- `SetOperationSuccessResponse::status` accepts any `u16` and later inserts it as a success response (`crates/gnr8-core/src/sdk/builtins.rs:442-445`, `crates/gnr8-core/src/sdk/builtins.rs:514-521`).

Recommendation: validate `base_path` when applying the transform: empty or `/` is acceptable, otherwise it should start with `/` and should not contain `?`, `#`, or traversal-like segments. Validate success statuses to `200..300`, or rename the API if it intentionally supports non-2xx response overrides.

### 7. Medium: response-less operations are documented as valid OpenAPI

Impact: the YAML writer emits `responses: {}` for operations without responses. This avoids YAML nulls, but OpenAPI tooling commonly expects at least one response entry. The current comment calls the empty map valid, which can hide spec compliance issues.

Evidence:

- The writer emits `responses: {}` when `responses.is_empty()` (`crates/gnr8-core/src/lower/yaml.rs:134-141`).
- The test explicitly asserts this behavior and calls it "valid" (`crates/gnr8-core/src/lower/mod.rs:1009-1018`).

Recommendation: generate a default response object for response-less operations, or reject response-less operations during lowering with a typed error. If the product intentionally supports incomplete specs, soften the comment and test wording so it does not claim full OpenAPI validity.

### 8. Medium: hot no-op checks use metadata stamps, not content hashes

Impact: the CLI can skip the child pipeline if input and output length plus mtime match the last verified stamp. That is fast, but it can miss same-length content edits where mtimes are preserved or coarsened by tooling/filesystems.

Evidence:

- `FileStamp` stores only path, length, and modification timestamp (`crates/gnr8-core/src/sdk/mod.rs:92-100`).
- `pre_child_verified_noop` trusts equality of collected input and output stamps (`crates/gnr8/src/main.rs:419-442`).
- Output stamps are collected from artifact paths and output anchors without content hashing (`crates/gnr8/src/main.rs:517-530`).

Recommendation: include a content hash for output artifacts in the verified no-op stamp, at least for generated artifact files. A two-tier strategy can preserve speed: use metadata as the quick filter, then hash only when metadata matches and the command is about to skip execution.

### 9. Medium/Low: `StaticFiles::from` promises project-relative paths but does not enforce them

Impact: `StaticFiles::from("../outside")` or an absolute path can read files outside the project before lifecycle write-path checks ever run. Code-as-config is trusted code, but the API contract says project-relative, and the current implementation does not enforce that contract.

Evidence:

- `StaticFiles::from` stores the source directory string without validation (`crates/gnr8-core/src/sdk/builtins.rs:907-911`).
- `generate` joins that string to `project_root` and reads from it (`crates/gnr8-core/src/sdk/builtins.rs:946-963`).
- Validation exists for include names only (`crates/gnr8-core/src/sdk/builtins.rs:1463-1474`, `crates/gnr8-core/src/sdk/builtins.rs:1522-1526`).

Recommendation: validate `from_dir` and `to_dir` with the same path rules used for frame names/output paths, then canonicalize or reject absolute/traversing source dirs. Add tests for absolute, empty, and `..` source directories.

### 10. Low: `package_name` uses ad hoc TOML parsing

Impact: the fast child-binary path can be disabled incorrectly for valid manifests that use TOML features the line parser does not understand, such as single-quoted strings or comments. The fallback to `cargo run` makes this mostly a performance and maintainability issue.

Evidence:

- `fresh_child_binary` depends on `package_name` (`crates/gnr8/src/child.rs:241-246`).
- `package_name` scans lines and splits on `=` instead of using a TOML parser or `cargo metadata` (`crates/gnr8/src/child.rs:295-316`).

Recommendation: prefer `cargo metadata` for this path, or add a tiny TOML parser dependency if startup cost is acceptable. If avoiding dependencies is more important, document the fallback and add tests for commented and single-quoted package names.

### 11. Low: one production `unreachable!` remains in Go SDK identifier uniquing

Impact: the loop is effectively unbounded, but this is still a panic-equivalent macro in production code. It is at odds with the repo's "no production panics" standard.

Evidence:

- `unique_ident` ends with `unreachable!("unbounded suffix loop must return")` (`crates/gnr8-core/src/gosdk/emit.rs:486-496`).

Recommendation: rewrite as an explicit `loop` with a checked suffix increment returning a typed `SdkGen` error if the suffix space is exhausted. That keeps even impossible states inside the typed-error model.

### 12. Low: stale phase-era documentation remains in public crate comments

Impact: comments and public docs say the core still contains Phase 1 stubs, while the implementation is much more complete. This undermines trust in otherwise careful docs.

Evidence:

- `crates/gnr8-core/src/lib.rs:1-3` says Phase 1 only stubs module seams.
- `crates/gnr8-core/src/error.rs:8-15` says Phase 1 ships only `NotYetImplemented`.

Recommendation: update crate-level and error docs to describe the current architecture. Also consider removing or de-emphasizing `not_yet` once no public seam uses it.

## Suggested Fix Order

1. Fix artifact cache invalidation for `StaticFiles`, because stale generated files are hard to diagnose and can ship silently.
2. Add shared schema-name uniqueness validation before OpenAPI and Go generation.
3. Decide and enforce multi-2xx SDK semantics.
4. Quote all YAML keys and add weird-key OpenAPI tests.
5. Decide the neutral map-key contract and reject or faithfully lower unsupported maps.
6. Tighten transform validation for base paths, success statuses, and static source directories.
7. Replace metadata-only verified no-op trust with content hashes on the skip path.
8. Clean up low-risk Rust polish: TOML parsing, `unreachable!`, stale phase docs.

## Test Gaps To Add

- Artifact-cache invalidation test for `StaticFiles` content edits.
- OpenAPI and Go SDK duplicate schema-name rejection tests.
- Multi-success operation test that returns `200`, `201`, and `204` across Go/Python/TypeScript clients.
- YAML key quoting tests for component, property, and security scheme keys.
- Non-string Go map key fixture and expected typed diagnostic.
- Base-path validation tests for `"books"`, `"/books"`, `"/"`, `""`, and path strings containing query/fragment characters.
- Verified no-op test that mutates an output to same byte length and preserved mtime.
