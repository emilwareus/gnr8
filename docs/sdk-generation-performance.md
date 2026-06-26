# SDK Generation Performance Notes

Date: 2026-06-26

Benchmark target: a realistic external Go backend with split Go, Python, and TypeScript SDK output. The target name and path are intentionally omitted.

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

