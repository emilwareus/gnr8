# Security policy

## Supported versions

gnr8 is an early release candidate. Security fixes are applied to the latest released
version.

## Report a vulnerability

Do not open a public issue for a suspected vulnerability.

Request a private reporting channel through the [OAIZ contact page](https://oaiz.io/).
Do not include vulnerability details or secrets in the first message. After a maintainer
gives you a private channel, include:

- the affected version or commit;
- the impact;
- steps to reproduce the problem; and
- a suggested fix, if you have one.

The maintainers will confirm receipt, assess the report, and coordinate a fix and
disclosure with you. Do not include real API keys, customer data, or other secrets in
the report.

For normal bugs and support questions, use the public issue tracker.

## Scope

`gnr8 generate` builds and runs the Rust crate in `.gnr8/` from your repository, with
the privileges of the user who invokes it. It is not sandboxed. This is the documented
model, not a flaw: the pipeline is code so that you can extend it.

Two flags exist for callers that will not accept it. `gnr8 --no-build` refuses to invoke
cargo, and `gnr8 --no-execute` refuses to build or run.

Reports worth sending:

- writing outside the declared target paths, or outside `.gnr8/`;
- executing code from a source the documentation does not name, including through a
  source frontend reading an analysed project;
- a credential or file content leaking into a generated artifact, a diagnostic, or the
  `.gnr8` cache;
- `--no-build` or `--no-execute` failing to prevent the thing it names.
