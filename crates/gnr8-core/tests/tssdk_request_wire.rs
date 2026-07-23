//! Runtime wire-contract coverage for generated TypeScript request parameters.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

const TSC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tsextract/node_modules/typescript/bin/tsc"
);

fn toolchain_available() -> bool {
    Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
        && Path::new(TSC).is_file()
}

fn unique_temp_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-tssdk-request-wire-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create request-wire temp dir");
    dir
}

fn parameter_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(
        r#"{
          "module": "wire.test",
          "operations": [
            {
              "id": "sendWire",
              "method": "GET",
              "path": "/wire",
              "handler": "sendWire",
              "params": [
                {
                  "name": "statuses",
                  "location": "query",
                  "required": true,
                  "schema": {
                    "type": "array",
                    "of": { "type": "primitive", "of": { "prim": "string" } }
                  },
                  "provenance": { "file": "wire.ts", "start_line": 1, "end_line": 1 }
                },
                {
                  "name": "redirect",
                  "location": "query",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "allow_reserved": true,
                  "provenance": { "file": "wire.ts", "start_line": 2, "end_line": 2 }
                },
                {
                  "name": "X-Signature",
                  "location": "header",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "wire.ts", "start_line": 3, "end_line": 3 }
                },
                {
                  "name": "session",
                  "location": "cookie",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "wire.ts", "start_line": 4, "end_line": 4 }
                },
                {
                  "name": "strict",
                  "location": "query",
                  "required": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "wire.ts", "start_line": 5, "end_line": 5 }
                }
              ],
              "request_body": null,
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "wire.ts", "start_line": 1, "end_line": 5 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/api",
          "title": "Wire API",
          "security": []
        }"#,
    )
    .expect("request parameter graph must deserialize")
}

fn command_output(mut command: Command) -> Result<(), String> {
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let mut diagnostics = String::from_utf8_lossy(&output.stdout).into_owned();
    diagnostics.push_str(&String::from_utf8_lossy(&output.stderr));
    Err(diagnostics)
}

const DRIVER: &str = r#"import { Client } from "./client";

const transport: typeof fetch = async (input, init) => {
  const url = new URL(String(input));
  const headers = new Headers(init?.headers);
  const statuses = url.searchParams.getAll("statuses");
  if (statuses.join(",") !== "active,pending") throw new Error(`statuses=${statuses}`);
  if (url.searchParams.has("X-Signature")) throw new Error(`signature leaked: ${url.search}`);
  if (url.searchParams.has("session")) throw new Error(`cookie leaked: ${url.search}`);
  if (!url.search.includes("redirect=https://example.test/a%20b+c?x=1")) {
    throw new Error(`redirect was not allowReserved: ${url.search}`);
  }
  if (!url.search.includes("strict=https%3A%2F%2Fstrict.test%2Fa%3Fx%3D1")) {
    throw new Error(`strict query was not encoded: ${url.search}`);
  }
  if (headers.get("X-Signature") !== "sig") throw new Error("missing signature");
  if (headers.get("Cookie") !== "session=session%2Fwith%20space%2Bplus") {
    throw new Error(`wrong cookie: ${headers.get("Cookie")}`);
  }
  return new Response(null, { status: 204 });
};

async function main(): Promise<void> {
  const client = new Client({ baseUrl: "https://api.test", fetch: transport });
  await client.sendWire(
    ["active", "pending"],
    "https://example.test/a b+c?x=1",
    "sig",
    "session/with space+plus",
    "https://strict.test/a?x=1",
  );
}

void main().catch((error: unknown) => {
  console.error(error);
  throw error;
});
"#;

#[test]
fn generated_typescript_request_parameters_match_the_wire_contract() {
    if !toolchain_available() {
        eprintln!("skipping TypeScript request wire test: node/tsc unavailable");
        return;
    }

    let graph = parameter_graph();
    let bundle = gnr8::tssdk::generate(&graph, "wireapi", &graph.base_path)
        .expect("generate TypeScript request-wire SDK");
    let dir = unique_temp_dir();
    gnr8::sdk::bundle::write_to_dir(&bundle, &dir).expect("materialize TypeScript SDK");
    std::fs::write(dir.join("driver.ts"), DRIVER).expect("write TypeScript driver");

    let mut compile = Command::new("node");
    compile
        .args([
            TSC,
            "--strict",
            "--target",
            "es2022",
            "--module",
            "commonjs",
            "--moduleResolution",
            "node",
            "--lib",
            "es2022,dom",
            "--outDir",
            "dist",
            "client.ts",
            "errors.ts",
            "models.ts",
            "driver.ts",
        ])
        .current_dir(&dir);
    assert_eq!(
        command_output(compile),
        Ok(()),
        "generated TypeScript request-wire driver must compile"
    );

    let mut run = Command::new("node");
    run.arg("dist/driver.js").current_dir(&dir);
    assert_eq!(
        command_output(run),
        Ok(()),
        "generated TypeScript request-wire driver must pass"
    );

    let _ = std::fs::remove_dir_all(dir);
}
