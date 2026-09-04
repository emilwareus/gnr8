# Contributing to gnr8

Thank you for your interest in gnr8. OAIZ maintains gnr8 as an open-source OAIZ Labs project.

## Before you start

Read the [engineering invariants](AGENTS.md). They define the product boundary. A change must keep
one native gnr8 contract from typed source extraction through generated artifacts.

Open an issue before you start a large change. State the problem, the proposed result, and the parts
of the pipeline that the change affects. Small fixes can go directly to a pull request.

## Development setup

The complete test suite uses these tools:

- Rust 1.85 or newer, with `rustfmt` and Clippy.
- Go, for the Go extractor and Go SDK tests.
- Python 3, for the Python extractor and generated SDK tests.
- Node and npm, for the TypeScript extractor tests.
- Ruby, for the YAML emitter cross-parser test (any 3.x with stdlib Psych; the test skips
  automatically when `ruby` is not on `PATH`).

Restore the TypeScript test toolchain:

```bash
make tsextract-deps
```

Build the CLI:

```bash
cargo build -p gnr8-cli
```

Run the full repository gate:

```bash
make check
```

Use a focused command while you work:

```bash
cargo test -p gnr8
cargo test -p gnr8-engine
cargo test -p gnr8-cli
make goextract-build
make pyextract-test
make tsextract-test
make invariants
```

## Change requirements

- Keep product behavior deterministic and sort output before emission.
- Return typed errors from production library code. Do not add `unwrap`, `expect`, or `panic` paths.
- Keep the internal API graph as the source of truth.
- Put new product behavior in repository-owned code.
- Do not make gnr8 read another generator’s annotations, configuration, or generated output.
- Add or update tests for all behavior changes.
- Update user documentation when a command, public Rust API, supported source pattern, or generated
  output changes.
- Do not edit generated example output by hand. Change the source or `.gnr8/` pipeline, then run
  `gnr8 generate`.

## Documentation style

Use short sentences and direct language. Verify commands, file paths, and public symbols against the
current code. Put detailed reference material in `docs/` and keep the root README focused on product
purpose, installation, and the first successful run.

## Pull requests

Keep each pull request focused. Explain:

1. What problem the change solves.
2. What behavior changes.
3. How you tested it.
4. Whether generated artifacts or public documentation changed.

All required checks must pass before merge. A maintainer can ask for changes when a proposal conflicts
with the engineering invariants, even when its tests pass.

## Conduct

Be respectful and constructive. Focus reviews on the work, give clear reasons for requested changes,
and assume good intent. Harassment and discriminatory behavior are not accepted.

## Reporting a vulnerability

Do not open a public issue. See [SECURITY.md](SECURITY.md).

## License

By submitting a contribution, you agree that it is licensed under the repository’s
[MIT License](LICENSE).
