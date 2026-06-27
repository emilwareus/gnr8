//! gnr8 generation lifecycle for the Flask bookstore example. This file IS the config — edit it to
//! adapt how the API is parsed and how the OpenAPI document + Python SDK are generated. It is an
//! ordinary Rust binary that composes a `Pipeline` and hands it to the gnr8 runner; the runner parses
//! argv (`__emit` / `__inspect`) and prints a JSON bundle to stdout.
//!
//! Run it from the example root so `Flask::new().inputs(["."])` analyzes the `app/` package here.
//! `.gnr8/` is excluded from language detection so the tree reads as Python:
//!
//! ```sh
//! cd examples/flask-bookstore
//! gnr8 generate
//! ```
//!
//! This reproduces the committed `examples/flask-bookstore/generated/` output. Every setting is a method
//! call below — there is no `config.toml`:
//!   inputs            → Flask::new().inputs(["."])       (the static `app/` package; never executed)
//!   base_path         → SetBasePath::new("/orders")      (the Blueprint url_prefix — a runtime value,
//!                                                          never folded into the code-derived path)
//!   title             → SetTitle::new("Bookstore Orders API")
//!   output.openapi    → OpenApi31::new().to("generated/openapi.yaml")
//!   output.sdk + module → PySdk::new().module("example.com/orders/sdk").to("generated/sdk")
//! plus a Header post-process that stamps the generated banner on every .py file.
//!
//! This is the HONEST Flask typed-envelope (the second-class Python frontend): typed handlers + typed
//! DTOs become facts; genuinely untyped surfaces (raw `request.json`, unannotated `request.args.get`)
//! emit DIAGNOSTICS rather than guessed facts (rule 3) — so the generated OpenAPI/SDK cover exactly the
//! typed surface. The app is parsed STATICALLY (pyextract reads the `ast`; it never imports or runs the
//! app), so no `pip install` is needed. There is no auth in the source, so no `ApplySecurity` stage.

use gnr8::sdk::prelude::*;

fn main() -> std::process::ExitCode {
    gnr8::runner::run(
        Pipeline::new()
            .source(Flask::new().inputs(["."]))
            .transform(SetBasePath::new("/orders"))
            .transform(SetTitle::new("Bookstore Orders API"))
            .target(OpenApi31::new().to("generated/openapi.yaml"))
            .target(PySdk::new().module("example.com/orders/sdk").to("generated/sdk"))
            .post(Header::generated()),
    )
}
