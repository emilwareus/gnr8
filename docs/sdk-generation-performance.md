# SDK Generation Performance Notes

Date: 2026-06-26

Benchmark target: a large Go API with split Go, Python, and TypeScript SDK output. Project names and paths are intentionally omitted.

Machine: macOS, 16 logical CPUs.

## Benchmark Shape

The realistic target currently emits:

- 480 Go source files
- 288 Python files
- 864 TypeScript files
- about 2,500 generated files across all configured outputs
- artifact bundle size: about 2.66 MB JSON over the host/child boundary

Measured commands:

- Isolated host write path: `gnr8 --json generate`
- Project-local child emission: `cargo run --quiet --manifest-path .gnr8/Cargo.toml -- __emit`
- Project-local inspection only: `cargo run --quiet --manifest-path .gnr8/Cargo.toml -- __inspect`
- Real make target: `make generate-sdks-core`

The real make target is not a pure gnr8 benchmark because it runs frontend formatting after generation.

## Baseline

Before optimizing SDK generation:

- Warm `gnr8 --json generate`: about 4.2-4.6s
- Warm child `__emit`: about 3.46-3.86s
- Warm child `__inspect`: about 1.84-1.94s
- Direct Go extraction alone: about 1.36-1.79s
- Full make target from earlier migration work: about 18.53s

The first profile split was clear: Go package loading/extraction is the irreducible front half, and Go SDK formatting dominated the back half once split output produced hundreds of files.

## Changes Made

1. Parallelized multi-file Go formatting.

   Root cause: split Go SDK generation spawned `gofmt` once per generated file, sequentially. The benchmark emits 480 Go files, so process startup was paid hundreds of times.

   Impact after this step:

   - Warm host generation: about 4.4s -> about 2.32s
   - Warm child emission: about 3.6s -> about 2.30s

2. Added direct file-generation APIs for built-in SDK targets.

   Root cause: the built-in targets generated an internal framed SDK bundle string, then parsed it back into files immediately. That added avoidable stringify/parse/allocation work, especially with split layouts.

   Impact: smaller than the formatter fix, but it removes unnecessary work from every split SDK target and keeps the target implementation aligned with the file-oriented output model.

3. Batched Go formatting into a single `gofmt -w` process.

   Root cause: even parallel formatting still spawned hundreds of `gofmt` processes. The new path writes a temporary tree, runs one `gofmt -w` over the generated files, then reads the canonical contents back in deterministic input order.

   Final warm timings:

   - `__inspect`: avg 1.745s, min 1.700s, max 1.830s
   - `__emit`: avg 2.205s, min 2.060s, max 2.290s
   - `gnr8 --json generate`: avg 2.075s, min 2.020s, max 2.130s

   Incremental impact over the parallel formatter:

   - Warm host generation: about 2.32s -> about 2.08s
   - Warm child emission: about 2.30s -> about 2.21s

## Final Impact

Compared with the initial benchmark:

- Warm host generation is now about 2.1s, down from about 4.4s: roughly 53% faster.
- Warm child emission is now about 2.2s, down from about 3.6s: roughly 39% faster.
- Full make target is now about 10.67s, down from about 18.53s in the earlier migration measurement: roughly 42% faster, though this includes frontend formatting after gnr8 finishes.

Validation after the final change:

- `cargo test -p gnr8-core --lib`
- `cargo build -p gnr8`
- Generated Python SDK `py_compile` plus Pydantic v2 import/model validation
- Generated Go SDK `go test ./...`
- Generated TypeScript SDK `tsc --noEmit`
- Target version validation script

## Remaining Bottlenecks

The main remaining cost is not SDK text emission. `__inspect` is about 1.7-1.8s warm, while full `__emit` is about 2.2s warm. That leaves only about 0.4-0.5s for all target generation, JSON serialization, and host writing.

Remaining time is mostly:

- Go package loading and type extraction through `go/packages`
- Starting the project-local `.gnr8` Cargo child process
- Serializing/deserializing the artifact bundle across stdout
- Hashing/comparing about 2,500 files during no-op host writes
- Frontend formatting in the benchmark target's makefile, which dominates full make timing after gnr8 is optimized

Possible future work:

- Persistent extraction/helper process for watch mode so Go package/type state can be reused.
- A safe explicit child build/run mode that avoids repeated `cargo run` overhead without accidentally running stale generator code.
- Incremental generation/write support keyed by extractor facts and target config.
- Optional timing instrumentation in `Pipeline::run` so source, transforms, targets, posts, serialization, and lifecycle writes can be measured without external scripts.

## Cache Iteration

After the SDK shape migration, the realistic target grew substantially:

- about 6,300 generated artifacts across OpenAPI, Go, Python, and TypeScript outputs
- compact child bundle before this iteration: about 23.4 MB
- steady-state `gnr8 generate --force`: about 20.10s
- steady-state child `__emit`: about 18.60s
- steady-state child `__inspect`: about 3.32s

The new dominant cost was target emission and repeatedly moving a large artifact bundle through the
host/child JSON boundary.

Changes in this iteration:

1. Added a content-keyed Go source graph cache under `.gnr8/cache/sources/go-gin/`.

   The cache key includes:

   - gnr8-core version
   - the configured package pattern list
   - a stable hash of source input files

   This skips `go/packages` extraction when source inputs are unchanged while still rerunning transforms
   and target generation when `.gnr8` config changes.

2. Added a content-keyed artifact cache under `.gnr8/cache/artifacts/`.

   The cache key includes:

   - the post-transform frozen IR hash
   - `.gnr8/src`, `.gnr8/Cargo.toml`, `.gnr8/Cargo.lock`
   - the compiled child binary fingerprint

   If the frozen IR and generation surface are unchanged, target generation is skipped entirely.

3. Added a compact child protocol for artifact-cache hits.

   On a warm artifact-cache hit, the child emits an artifact cache reference instead of the full
   generated file payload. The host then loads the cached artifacts locally before running the existing
   manifest/write lifecycle. This keeps the host as the only writer while avoiding the largest stdout
   transfer from the child.

4. Cached the Go extraction sidecar binary.

   The sidecar is built once into a content-keyed temp directory based on the goextract source hash,
   then reused instead of paying `go run .` startup/build overhead on every extraction miss.

5. Parallelized host lifecycle pre-reads.

   The pure write-planning policy is unchanged. The real filesystem path now pre-reads generated output
   files in worker threads, then feeds the same `plan_writes` truth table from memory.

Measured impact on the realistic target:

| Scenario | Before | After |
| --- | ---: | ---: |
| Warm `gnr8 generate --force` | 20.10s | 3.01s |
| Warm child `__emit` | 18.60s | 1.93s |
| Warm child `__inspect` | 3.32s | 1.67s |
| Child `__emit` stdout | 23.4 MB | 0.92 MB |

The first run after a gnr8 rebuild or source/config change still refills caches and remains comparable
to the uncached path. The edit loop now has three tiers:

- no source/config/IR change: target emission is skipped and the child sends only a cache reference
- source changed but frozen IR is identical: extraction runs, target emission is skipped
- API shape changed: extraction and targets rerun, then the new artifact cache is populated

Remaining opportunities:

- Store artifact hash metadata separately so a no-op host lifecycle can avoid reading the full cached
  artifact payload unless a write is required.
- Add a persistent watch-mode child process to avoid repeated Cargo/process startup entirely.
- Add source-level incremental extraction for Go package subsets instead of hashing/loading the full
  configured source input.

## Hot No-Op Iteration

The next pass focused on the realistic edit loop: repeated generation after no source, config, or
output changes. The goal was to stop doing work just to prove no work was needed.

Changes:

1. Artifact metadata cache

   Each artifact cache entry now has a sidecar metadata file containing only generated path and blake3
   hash. On compact cache hits, the host can classify unchanged/protected files from metadata and only
   loads the full generated text if a file must actually be written.

2. Metadata lifecycle planner

   `plan_metadata_writes`, `plan_only_cached`, and `regenerate_cached_with_anchors` mirror the existing
   write truth table without materializing generated bytes. The forced overwrite path deliberately
   falls back to full artifacts when a protected file needs to be rewritten.

3. Cached file hashing

   File content hashes are cached by path, byte length, and modification timestamp. Input/config hashes
   and generated-output hashes use separate cache files so the child does not pay to parse thousands of
   output entries when it only needs source/config fingerprints.

4. Direct child execution

   The host runs the already-built `.gnr8` child binary directly when it is newer than `.gnr8/src`,
   `.gnr8/Cargo.toml`, `.gnr8/Cargo.lock`, and the current gnr8 executable. Cargo remains the fallback
   whenever anything changed.

5. Verified no-op stamp

   After a clean no-op generation, the host records a stamp of the generated output tree. The next run
   can compare path/length/mtime stamps and skip output hash verification entirely. Any output-tree
   change misses the stamp and falls back to hash verification.

Measured impact on the same realistic target:

| Scenario | Previous best | After |
| --- | ---: | ---: |
| Warm `gnr8 generate --force` | 3.01s | 0.83-0.86s typical |
| Warm child `__emit` | 1.93s | 0.50s direct child |
| Warm `gnr8 check` | 2.09s | 1.08-1.31s |
| Child `__emit` stdout | 0.92 MB | 0.92 MB |

Invalidation probe:

- Touched one generated OpenAPI output without changing bytes.
- The next `gnr8 generate --force` missed the verified stamp, fell back to hash verification, and
  reported `0 written, 6298 unchanged, 0 deleted, 0 skipped` in 1.66s.
- The following run used the restamped fast path again and completed in 0.84s.

Validation after this pass:

- `cargo test`
- `go test ./...` in the extraction sidecar
- generated Go SDK `go test ./...`
- generated TypeScript SDK `npm run build --silent`
- generated Python SDK `uv run --group dev pytest -q`
- realistic target backend `go test ./...`

Remaining opportunities:

- Watch mode can go further by keeping a long-lived child/process context and sending incremental
  invalidation events instead of starting a child process per run.
- Source-level incremental extraction could reduce the 0.50s direct-child floor when only a small Go
  package subset changed.
- A compact diagnostics mode would reduce the remaining 0.92 MB child payload on large APIs.

## Pre-Child No-Op Iteration

The previous fast path still started the child on every run. The next improvement was to let the host
prove a no-op before process startup.

Changes:

1. Source input roots

   `Source` now has an opt-in `cache_input_roots` hook. Built-in Go/Gin sources declare their configured
   source root, while custom sources default to conservative behavior and keep using the child path.

2. Child-emitted input stamps

   The artifact bundle now carries source input roots and metadata-only file stamps. The host combines
   those with `.gnr8` config files and the current host executable when writing the verified no-op stamp.

3. Host pre-child verification

   On the next run, the host rescans the recorded input roots and output tree by path/length/mtime. If
   both match the stamp, `gnr8 generate` returns the no-op outcome without starting the child process,
   deserializing diagnostics, loading artifacts, or hashing generated outputs.

4. Scratch-directory exclusion

   `.context` is now excluded from source/cache scans alongside `.git`, `.gnr8`, `node_modules`,
   `target`, `vendor`, and `__pycache__`. This avoids invalidating generation from workspace scratch
   files under a source root.

Measured impact on the same realistic target:

| Scenario | Previous best | After |
| --- | ---: | ---: |
| Hot no-op `gnr8 generate --force` | 0.83-0.86s | 0.19-0.23s |
| Hot no-op `gnr8 check` | 1.08-1.31s | 0.21s |
| After source mtime touch | 0.92-1.24s for the invalidating run, then 0.20s again |

Invalidation probes:

- Touching a real source file missed the pre-child path, ran the child/cache verification path, reported
  `0 written, 6298 unchanged, 0 deleted, 0 skipped`, refreshed the input stamp, and the following run
  returned to 0.20s.
- Touching a generated output still misses the output stamp and falls back to hash verification before
  restamping.

Validation after this pass:

- `cargo test`
- `go test ./...` in the extraction sidecar
- generated Go SDK `go test ./...`
- generated TypeScript SDK `npm run build --silent`
- generated Python SDK `uv run --group dev pytest -q`
- realistic target backend `go test ./...`
