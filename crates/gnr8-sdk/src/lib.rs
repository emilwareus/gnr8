//! gnr8 — the code-as-config SDK a project-local `.gnr8/` crate links.
//!
//! The installed `gnr8` CLI scaffolds a small Rust binary crate at `.gnr8/`. That crate depends on
//! **this** package, imports [`sdk::prelude`], builds a [`sdk::Pipeline`], and hands it to
//! [`worker::run`]. The CLI compiles that crate once, runs the resulting binary, and talks to it
//! over a framed protocol.
//!
//! ```no_run
//! use gnr8::sdk::prelude::*;
//!
//! fn main() -> std::process::ExitCode {
//!     gnr8::worker::run(
//!         Pipeline::new()
//!             .source(FastApi::new().inputs(["."]))
//!             .transform(SetBasePath::new("/api"))
//!             .transform(SetTitle::new("Public API"))
//!             .transform(ApplySecurity::api_key("ApiKeyAuth", "X-API-Key"))
//!             .target(OpenApi31::new().to("generated/openapi.yaml"))
//!             .target(PySdk::new().module("example.com/public/sdk").to("generated/sdk"))
//!             .post(Header::generated()),
//!     )
//! }
//! ```
//!
//! ## What is in this crate, and what is not
//!
//! This crate carries the API graph ([`graph::ApiGraph`]), the four stage traits, the [`sdk::Pipeline`]
//! container, the built-in stage **declarations** ([`sdk::builtins`]), and the host↔worker
//! [`protocol`]. Its whole dependency list is `serde`, `serde_json`, `blake3` and `thiserror`.
//!
//! Source extraction, `OpenAPI` lowering, SDK emission, the ownership manifest and the filesystem
//! writer live in the host engine, compiled once into the installed `gnr8` binary. A built-in stage
//! in your pipeline is a declaration the host executes; only the stages you wrote — wrapped in
//! [`sdk::Custom`] — run in your process. That is why upgrading gnr8 does not rebuild a code
//! generator inside every project that uses it.
//!
//! For agent-facing CLI workflows, run `gnr8 guide` or start with the
//! <https://github.com/emilwareus/gnr8/blob/main/docs/agents/index.md> task index.

pub mod error;
pub use error::Error;

pub mod facts;
pub mod graph;
pub mod protocol;
pub mod sdk;
pub mod worker;

/// Convenience alias for [`sdk::prelude`], so `use gnr8::prelude::*;` also works.
pub use sdk::prelude;
