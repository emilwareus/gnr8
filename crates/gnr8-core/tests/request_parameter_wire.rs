//! Cross-language wire contract for inferred request parameters.
//!
//! The same neutral graph is emitted as Go, TypeScript, and Python. Each generated client is then
//! executed against a local/injected transport and must produce the same query, header, cookie, and
//! security facts. Toolchains are optional so a focused developer environment can still run the rest
//! of the suite; CI installs all three.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

const TSC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tsextract/node_modules/typescript/bin/tsc"
);

const PARAMETER_GRAPH_JSON: &str = r#"{
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
                  "style": "form",
                  "explode": true,
                  "provenance": { "file": "wire.go", "start_line": 1, "end_line": 1 }
                },
                {
                  "name": "redirect",
                  "location": "query",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "allow_reserved": true,
                  "provenance": { "file": "wire.go", "start_line": 2, "end_line": 2 }
                },
                {
                  "name": "X-Signature",
                  "location": "header",
                  "required": true,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "wire.go", "start_line": 3, "end_line": 3 }
                },
                {
                  "name": "filters",
                  "location": "query",
                  "required": false,
                  "schema": {
                    "type": "map",
                    "of": {
                      "key": { "type": "primitive", "of": { "prim": "string" } },
                      "value": { "type": "primitive", "of": { "prim": "string" } }
                    }
                  },
                  "style": "deepObject",
                  "explode": true,
                  "provenance": { "file": "wire.go", "start_line": 4, "end_line": 4 }
                },
                {
                  "name": "X-Tags",
                  "location": "header",
                  "required": false,
                  "schema": {
                    "type": "array",
                    "of": { "type": "primitive", "of": { "prim": "string" } }
                  },
                  "style": "simple",
                  "explode": false,
                  "provenance": { "file": "wire.go", "start_line": 5, "end_line": 5 }
                },
                {
                  "name": "session",
                  "location": "cookie",
                  "required": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "wire.go", "start_line": 6, "end_line": 6 }
                },
                {
                  "name": "force",
                  "location": "query",
                  "required": false,
                  "schema": { "type": "primitive", "of": { "prim": "bool" } },
                  "provenance": { "file": "wire.go", "start_line": 7, "end_line": 7 }
                },
                {
                  "name": "strict",
                  "location": "query",
                  "required": false,
                  "schema": { "type": "primitive", "of": { "prim": "string" } },
                  "provenance": { "file": "wire.go", "start_line": 8, "end_line": 8 }
                },
                {
                  "name": "free",
                  "location": "query",
                  "required": false,
                  "schema": {
                    "type": "map",
                    "of": {
                      "key": { "type": "primitive", "of": { "prim": "string" } },
                      "value": { "type": "primitive", "of": { "prim": "string" } }
                    }
                  },
                  "style": "form",
                  "explode": true,
                  "allow_reserved": true,
                  "provenance": { "file": "wire.go", "start_line": 9, "end_line": 9 }
                }
              ],
              "request_body": null,
              "responses": [ { "status": 204, "body": null } ],
              "provenance": { "file": "wire.go", "start_line": 1, "end_line": 9 }
            }
          ],
          "schemas": [],
          "diagnostics": [],
          "base_path": "/api",
          "title": "Wire API",
          "security": [
            {
              "id": "ApiKeyAuth",
              "kind": "apiKey",
              "location": "header",
              "name": "X-API-Key",
              "global": true
            },
            {
              "id": "QueryKeyAuth",
              "kind": "apiKey",
              "location": "query",
              "name": "api_key",
              "global": true
            }
          ]
        }"#;

fn parameter_graph() -> gnr8::graph::ApiGraph {
    serde_json::from_str(PARAMETER_GRAPH_JSON).expect("request parameter graph must deserialize")
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "gnr8-request-wire-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create request wire temp dir");
    dir
}

fn available(program: &str, arg: &str) -> bool {
    Command::new(program)
        .arg(arg)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

fn command_output(mut command: Command) -> Result<String, String> {
    let output = command.output().map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let mut diagnostics = String::from_utf8_lossy(&output.stdout).into_owned();
    diagnostics.push_str(&String::from_utf8_lossy(&output.stderr));
    Err(diagnostics)
}

fn write_bundle(bundle: &str, dir: &Path) {
    gnr8::sdk::bundle::write_to_dir(bundle, dir).expect("materialize generated SDK bundle");
}

fn write_artifacts(root: &Path, artifacts: &gnr8::sdk::Artifacts) {
    for artifact in artifacts.files() {
        let path = root.join(&artifact.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create generated artifact parent");
        }
        std::fs::write(path, &artifact.text).expect("write generated artifact");
    }
}

fn materialize_go_compat_sdk(graph: &gnr8::graph::ApiGraph) -> PathBuf {
    use gnr8::sdk::prelude::*;

    let root = unique_temp_dir("go-compat-root");
    let mut artifacts = Artifacts::new();
    GoSdk::new()
        .module("example.com/wireapi")
        .to("sdk")
        .profile(SdkProfile::go_openapi_generator_compat())
        .source_only()
        .generate(graph, &mut artifacts, &Cx::new(root.clone()))
        .expect("generate Go compatibility SDK");
    write_artifacts(&root, &artifacts);
    let dir = root.join("sdk");
    std::fs::write(dir.join("go.mod"), "module wiretest\n\ngo 1.26\n")
        .expect("write compatibility go.mod");
    dir
}

fn materialize_ts_compat_sdk(
    graph: &gnr8::graph::ApiGraph,
    profile: gnr8::sdk::profile::SdkProfile,
    label: &str,
) -> PathBuf {
    use gnr8::sdk::prelude::*;

    let root = unique_temp_dir(label);
    let mut artifacts = Artifacts::new();
    TsSdk::new()
        .module("wireapi")
        .to("sdk")
        .profile(profile)
        .without_docs()
        .generate(graph, &mut artifacts, &Cx::new(root.clone()))
        .expect("generate TypeScript compatibility SDK");
    write_artifacts(&root, &artifacts);
    root.join("sdk")
}

fn run_ts_project(dir: &Path) -> Result<String, String> {
    let mut compile = Command::new("node");
    compile
        .args([TSC, "--project", "tsconfig.json"])
        .current_dir(dir);
    command_output(compile)?;

    let mut run = Command::new("node");
    run.arg("dist/driver.js").current_dir(dir);
    command_output(run)
}

fn write_axios_stub(dir: &Path) {
    let axios_dir = dir.join("node_modules/axios");
    std::fs::create_dir_all(&axios_dir).expect("create Axios stub directory");
    std::fs::write(
        axios_dir.join("package.json"),
        r#"{"name":"axios","version":"1.0.0","main":"index.js","types":"index.d.ts"}"#,
    )
    .expect("write Axios stub package");
    std::fs::write(
        axios_dir.join("index.d.ts"),
        r"export interface AxiosRequestConfig {
  method?: string;
  url?: string;
  params?: unknown;
  data?: unknown;
  headers?: unknown;
  validateStatus?: (status: number) => boolean;
  [key: string]: unknown;
}
export interface AxiosResponse<T = unknown> {
  status: number;
  headers: Record<string, unknown>;
  data: T;
}
export interface AxiosInstance {
  request<T = unknown>(config: AxiosRequestConfig): Promise<AxiosResponse<T>>;
}
declare const axios: AxiosInstance;
export default axios;
",
    )
    .expect("write Axios stub types");
    std::fs::write(
        axios_dir.join("index.js"),
        r#"const axios = { request: async () => { throw new Error("unexpected default axios"); } };
module.exports = axios;
module.exports.default = axios;
"#,
    )
    .expect("write Axios stub runtime");
}

#[test]
fn go_request_parameters_match_the_wire_contract() {
    if !available("go", "version") {
        eprintln!("skipping Go request wire test: go toolchain unavailable");
        return;
    }
    let graph = parameter_graph();
    let bundle = gnr8::gosdk::generate(&graph, "wireapi", &graph.base_path)
        .expect("generate Go request wire SDK");
    let dir = unique_temp_dir("go");
    write_bundle(&bundle, &dir);
    std::fs::write(dir.join("go.mod"), "module wiretest\n\ngo 1.26\n")
        .expect("write hermetic go.mod");
    std::fs::write(dir.join("wire_test.go"), GO_WIRE_TEST).expect("write Go wire test");

    let mut command = Command::new("go");
    command
        .args(["test", "./..."])
        .current_dir(&dir)
        .env("GOPROXY", "off")
        .env("GOFLAGS", "-mod=mod");
    let result = command_output(command);
    assert!(result.is_ok(), "generated Go wire test failed: {result:?}");
    let _ = std::fs::remove_dir_all(dir);
}

const GO_WIRE_TEST: &str = r#"package wireapi

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestRequestParameterWireContract(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.URL.Query()["statuses"]; len(got) != 2 || got[0] != "active" || got[1] != "pending" {
			t.Errorf("statuses = %#v", got)
		}
		if got := r.URL.Query().Get("filters[a]"); got != "one" {
			t.Errorf("filters[a] = %q", got)
		}
		if got := r.URL.Query().Get("filters[z]"); got != "two" {
			t.Errorf("filters[z] = %q", got)
		}
		if got := r.URL.Query().Get("api_key"); got != "secret" {
			t.Errorf("api_key = %q", got)
		}
		if !strings.Contains(r.URL.RawQuery, "redirect=https://example.test/a?x=1") {
			t.Errorf("redirect was not allowReserved: %s", r.URL.RawQuery)
		}
		if got := r.URL.Query().Get("force"); got != "true" {
			t.Errorf("force = %q", got)
		}
		if !strings.Contains(r.URL.RawQuery, "strict=https%3A%2F%2Fstrict.test%2Fa%3Fx%3D1") ||
			!strings.Contains(r.URL.RawQuery, "strict=https://free.test/a?x=1") {
			t.Errorf("per-value allowReserved policy was lost: %s", r.URL.RawQuery)
		}
		if got := r.Header.Get("X-Signature"); got != "sig" {
			t.Errorf("X-Signature = %q", got)
		}
		if got := r.Header.Get("X-Tags"); got != "red,blue" {
			t.Errorf("X-Tags = %q", got)
		}
		if got := r.Header.Get("X-API-Key"); got != "secret" {
			t.Errorf("X-API-Key = %q", got)
		}
		if got := r.Header.Get("Cookie"); got != "session=session%2Fwith%20space%2Bplus" {
			t.Errorf("session cookie = %q", got)
		}
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	filters := map[string]string{"z": "two", "a": "one"}
	free := map[string]string{"strict": "https://free.test/a?x=1"}
	tags := []string{"red", "blue"}
	session := "session/with space+plus"
	force := true
	strict := "https://strict.test/a?x=1"
	client := NewClient(srv.URL, WithAPIKey("secret"))
	_, err := client.SendWire(context.Background(), SendWireParams{
		Statuses:   []string{"active", "pending"},
		Redirect:   "https://example.test/a?x=1",
		XSignature: "sig",
		Filters:    &filters,
		XTags:      &tags,
		Session:    &session,
		Force:      &force,
		Strict:     &strict,
		Free:       &free,
	})
	if err != nil {
		t.Fatalf("SendWire returned error: %v", err)
	}
}
"#;

#[test]
fn go_openapi_generator_compat_parameters_match_the_wire_contract() {
    if !available("go", "version") {
        eprintln!("skipping Go compatibility request wire test: go toolchain unavailable");
        return;
    }
    let graph = parameter_graph();
    let dir = materialize_go_compat_sdk(&graph);
    std::fs::write(dir.join("wire_test.go"), GO_COMPAT_WIRE_TEST)
        .expect("write Go compatibility wire test");

    let mut command = Command::new("go");
    command
        .args(["test", "./..."])
        .current_dir(&dir)
        .env("GOPROXY", "off")
        .env("GOFLAGS", "-mod=mod");
    let result = command_output(command);
    assert!(
        result.is_ok(),
        "generated Go compatibility wire test failed: {result:?}"
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
}

const GO_COMPAT_WIRE_TEST: &str = r#"package wireapi

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestCompatibilityRequestParameterWireContract(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.URL.Query()["statuses"]; len(got) != 2 || got[0] != "active" || got[1] != "pending" {
			t.Errorf("statuses = %#v", got)
		}
		if got := r.URL.Query().Get("filters[a]"); got != "one" {
			t.Errorf("filters[a] = %q", got)
		}
		if got := r.URL.Query().Get("filters[z]"); got != "two" {
			t.Errorf("filters[z] = %q", got)
		}
		if got := r.URL.Query().Get("api_key"); got != "secret" {
			t.Errorf("api_key = %q", got)
		}
		if !strings.Contains(r.URL.RawQuery, "redirect=https://example.test/a?x=1") {
			t.Errorf("redirect was not allowReserved: %s", r.URL.RawQuery)
		}
		if got := r.URL.Query().Get("force"); got != "true" {
			t.Errorf("force = %q", got)
		}
		if !strings.Contains(r.URL.RawQuery, "strict=https%3A%2F%2Fstrict.test%2Fa%3Fx%3D1") ||
			!strings.Contains(r.URL.RawQuery, "strict=https://free.test/a?x=1") {
			t.Errorf("per-value allowReserved policy was lost: %s", r.URL.RawQuery)
		}
		if got := r.Header.Get("X-Signature"); got != "sig" {
			t.Errorf("X-Signature = %q", got)
		}
		if got := r.Header.Get("X-Tags"); got != "red,blue" {
			t.Errorf("X-Tags = %q", got)
		}
		if got := r.Header.Get("X-API-Key"); got != "secret" {
			t.Errorf("X-API-Key = %q", got)
		}
		if got := r.Header.Get("Cookie"); got != "session=session%2Fwith%20space%2Bplus" {
			t.Errorf("session cookie = %q", got)
		}
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	configuration := NewConfiguration()
	configuration.Servers = ServerConfigurations{{URL: srv.URL}}
	configuration.HTTPClient = srv.Client()
	client := NewAPIClient(configuration)
	ctx := WithAPIKey(context.Background(), "ApiKeyAuth", APIKey{Key: "secret"})
	ctx = WithAPIKey(ctx, "QueryKeyAuth", APIKey{Key: "secret"})
	_, err := client.WireAPI.SendWire(ctx).
		Statuses([]string{"active", "pending"}).
		Redirect("https://example.test/a?x=1").
		XSignature("sig").
		Filters(map[string]string{"z": "two", "a": "one"}).
		XTags([]string{"red", "blue"}).
		Session("session/with space+plus").
		Force(true).
		Strict("https://strict.test/a?x=1").
		Free(map[string]string{"strict": "https://free.test/a?x=1"}).
		Execute()
	if err != nil {
		t.Fatalf("SendWire returned error: %v", err)
	}
}
"#;

#[test]
fn typescript_request_parameters_match_the_wire_contract() {
    if !available("node", "--version") || !Path::new(TSC).is_file() {
        eprintln!("skipping TypeScript request wire test: node/tsc unavailable");
        return;
    }
    let graph = parameter_graph();
    let bundle = gnr8::tssdk::generate(&graph, "wireapi", &graph.base_path)
        .expect("generate TypeScript request wire SDK");
    let dir = unique_temp_dir("typescript");
    write_bundle(&bundle, &dir);
    std::fs::write(dir.join("driver.ts"), TS_WIRE_DRIVER).expect("write TypeScript wire driver");

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
    let compiled = command_output(compile);
    assert!(
        compiled.is_ok(),
        "generated TypeScript wire driver did not compile: {compiled:?}"
    );

    let mut run = Command::new("node");
    run.arg("dist/driver.js").current_dir(&dir);
    let result = command_output(run);
    assert!(
        result.is_ok(),
        "generated TypeScript wire driver failed: {result:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

const TS_WIRE_DRIVER: &str = r#"import { Client } from "./client";

const transport: typeof fetch = async (input, init) => {
  const url = new URL(String(input));
  const headers = new Headers(init?.headers);
  const statuses = url.searchParams.getAll("statuses");
  if (statuses.join(",") !== "active,pending") throw new Error(`statuses=${statuses}`);
  if (url.searchParams.get("filters[a]") !== "one") throw new Error(url.search);
  if (url.searchParams.get("filters[z]") !== "two") throw new Error(url.search);
  if (url.searchParams.get("api_key") !== "secret") throw new Error(url.search);
  if (!url.search.includes("redirect=https://example.test/a?x=1")) throw new Error(url.search);
  if (url.searchParams.get("force") !== "true") throw new Error(url.search);
  if (!url.search.includes("strict=https%3A%2F%2Fstrict.test%2Fa%3Fx%3D1")) throw new Error(url.search);
  if (!url.search.includes("strict=https://free.test/a?x=1")) throw new Error(url.search);
  if (headers.get("X-Signature") !== "sig") throw new Error("missing signature");
  if (headers.get("X-Tags") !== "red,blue") throw new Error("wrong tags");
  if (headers.get("X-API-Key") !== "secret") throw new Error("missing API key");
  if (headers.get("Cookie") !== "session=session%2Fwith%20space%2Bplus") throw new Error("wrong cookie");
  return new Response(null, { status: 204 });
};

async function main(): Promise<void> {
  const client = new Client({ baseUrl: "https://api.test", fetch: transport, apiKey: "secret" });
  await client.sendWire(
    ["active", "pending"],
    "https://example.test/a?x=1",
    "sig",
    { z: "two", a: "one" },
    ["red", "blue"],
    "session/with space+plus",
    true,
    "https://strict.test/a?x=1",
    { strict: "https://free.test/a?x=1" },
  );
}

void main().catch((error: unknown) => {
  console.error(error);
  throw error;
});
"#;

#[test]
fn typescript_axios_compat_parameters_match_the_wire_contract() {
    if !available("node", "--version") || !Path::new(TSC).is_file() {
        eprintln!("skipping TypeScript Axios compatibility wire test: node/tsc unavailable");
        return;
    }
    let graph = parameter_graph();
    let dir = materialize_ts_compat_sdk(
        &graph,
        gnr8::sdk::profile::SdkProfile::openapi_generator_compat(),
        "typescript-axios-compat-root",
    );
    write_axios_stub(&dir);
    std::fs::write(dir.join("driver.ts"), TS_AXIOS_COMPAT_WIRE_DRIVER)
        .expect("write TypeScript Axios compatibility wire driver");

    let result = run_ts_project(&dir);
    assert!(
        result.is_ok(),
        "generated TypeScript Axios compatibility wire driver failed: {result:?}"
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
}

const TS_AXIOS_COMPAT_WIRE_DRIVER: &str = r#"import type { AxiosInstance, AxiosRequestConfig, AxiosResponse } from "axios";
import { Configuration, DefaultApi } from "./index";

function verify(config: AxiosRequestConfig): void {
  const url = new URL(String(config.url));
  const headers = new Headers(config.headers as HeadersInit);
  const statuses = url.searchParams.getAll("statuses");
  if (statuses.join(",") !== "active,pending") throw new Error(`statuses=${statuses}`);
  if (url.searchParams.get("filters[a]") !== "one") throw new Error(url.search);
  if (url.searchParams.get("filters[z]") !== "two") throw new Error(url.search);
  if (url.searchParams.get("api_key") !== "secret") throw new Error(url.search);
  if (!url.search.includes("redirect=https://example.test/a?x=1")) throw new Error(url.search);
  if (url.searchParams.get("force") !== "true") throw new Error(url.search);
  if (!url.search.includes("strict=https%3A%2F%2Fstrict.test%2Fa%3Fx%3D1")) throw new Error(url.search);
  if (!url.search.includes("strict=https://free.test/a?x=1")) throw new Error(url.search);
  if (headers.get("X-Signature") !== "sig") throw new Error("missing signature");
  if (headers.get("X-Tags") !== "red,blue") throw new Error("wrong tags");
  if (headers.get("X-API-Key") !== "secret") throw new Error("missing API key");
  if (headers.get("Cookie") !== "session=session%2Fwith%20space%2Bplus") throw new Error("wrong cookie");
}

const transport: AxiosInstance = {
  async request<T = unknown>(config: AxiosRequestConfig): Promise<AxiosResponse<T>> {
    verify(config);
    return { status: 204, headers: {}, data: undefined as T };
  },
};

async function main(): Promise<void> {
  const configuration = new Configuration({
    basePath: "https://api.test",
    apiKeys: { ApiKeyAuth: "secret", QueryKeyAuth: "secret" },
  });
  const api = new DefaultApi(configuration, undefined, transport);
  await api.sendWire({
    statuses: ["active", "pending"],
    redirect: "https://example.test/a?x=1",
    xSignature: "sig",
    filters: { z: "two", a: "one" },
    xTags: ["red", "blue"],
    session: "session/with space+plus",
    force: true,
    strict: "https://strict.test/a?x=1",
    free: { strict: "https://free.test/a?x=1" },
  });
}

void main().catch((error: unknown) => {
  console.error(error);
  throw error;
});
"#;

#[test]
fn typescript_fetch_compat_parameters_match_the_wire_contract() {
    if !available("node", "--version") || !Path::new(TSC).is_file() {
        eprintln!("skipping TypeScript Fetch compatibility wire test: node/tsc unavailable");
        return;
    }
    let graph = parameter_graph();
    let dir = materialize_ts_compat_sdk(
        &graph,
        gnr8::sdk::profile::SdkProfile::typescript_fetch_compat(),
        "typescript-fetch-compat-root",
    );
    std::fs::write(dir.join("driver.ts"), TS_FETCH_COMPAT_WIRE_DRIVER)
        .expect("write TypeScript Fetch compatibility wire driver");

    let result = run_ts_project(&dir);
    assert!(
        result.is_ok(),
        "generated TypeScript Fetch compatibility wire driver failed: {result:?}"
    );
    let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
}

const TS_FETCH_COMPAT_WIRE_DRIVER: &str = r#"import { Configuration, DefaultApi } from "./index";

const transport: typeof fetch = async (input, init) => {
  const url = new URL(String(input));
  const headers = new Headers(init?.headers);
  const statuses = url.searchParams.getAll("statuses");
  if (statuses.join(",") !== "active,pending") throw new Error(`statuses=${statuses}`);
  if (url.searchParams.get("filters[a]") !== "one") throw new Error(url.search);
  if (url.searchParams.get("filters[z]") !== "two") throw new Error(url.search);
  if (url.searchParams.get("api_key") !== "secret") throw new Error(url.search);
  if (!url.search.includes("redirect=https://example.test/a?x=1")) throw new Error(url.search);
  if (url.searchParams.get("force") !== "true") throw new Error(url.search);
  if (!url.search.includes("strict=https%3A%2F%2Fstrict.test%2Fa%3Fx%3D1")) throw new Error(url.search);
  if (!url.search.includes("strict=https://free.test/a?x=1")) throw new Error(url.search);
  if (headers.get("X-Signature") !== "sig") throw new Error("missing signature");
  if (headers.get("X-Tags") !== "red,blue") throw new Error("wrong tags");
  if (headers.get("X-API-Key") !== "secret") throw new Error("missing API key");
  if (headers.get("Cookie") !== "session=session%2Fwith%20space%2Bplus") throw new Error("wrong cookie");
  return new Response(null, { status: 204 });
};

async function main(): Promise<void> {
  const configuration = new Configuration({
    basePath: "https://api.test",
    fetchApi: transport,
    apiKeys: { ApiKeyAuth: "secret", QueryKeyAuth: "secret" },
  });
  const api = new DefaultApi(configuration);
  await api.sendWire({
    statuses: ["active", "pending"],
    redirect: "https://example.test/a?x=1",
    xSignature: "sig",
    filters: { z: "two", a: "one" },
    xTags: ["red", "blue"],
    session: "session/with space+plus",
    force: true,
    strict: "https://strict.test/a?x=1",
    free: { strict: "https://free.test/a?x=1" },
  });
}

void main().catch((error: unknown) => {
  console.error(error);
  throw error;
});
"#;

#[test]
fn python_request_parameters_match_the_wire_contract() {
    if !available("python3", "--version") {
        eprintln!("skipping Python request wire test: python3 unavailable");
        return;
    }
    let graph = parameter_graph();
    let bundle = gnr8::pysdk::generate_with_options(
        &graph,
        "wireapi",
        &graph.base_path,
        &gnr8::sdk::layout::SdkFileLayout::compact(),
        gnr8::sdk::model_style::PyModelStyle::Dataclass,
        &gnr8::sdk::surface::SdkTypeAliases::default(),
    )
    .expect("generate Python request wire SDK");
    let dir = unique_temp_dir("python");
    let package_dir = dir.join("wireapi");
    std::fs::create_dir_all(&package_dir).expect("create Python package dir");
    write_bundle(&bundle, &package_dir);
    std::fs::write(dir.join("driver.py"), PY_WIRE_DRIVER).expect("write Python wire driver");

    let mut command = Command::new("python3");
    command
        .arg("driver.py")
        .current_dir(&dir)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONNOUSERSITE", "1");
    let result = command_output(command);
    assert!(
        result.is_ok(),
        "generated Python wire driver failed: {result:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

const PY_WIRE_DRIVER: &str = r#"import urllib.parse

from wireapi import Client


class Response:
    status = 204
    headers = {}

    def __enter__(self):
        return self

    def __exit__(self, *args):
        return False

    def read(self):
        return b""


class Opener:
    def open(self, request, timeout=None):
        parsed = urllib.parse.urlsplit(request.full_url)
        query = urllib.parse.parse_qs(parsed.query)
        assert query["statuses"] == ["active", "pending"], query
        assert query["filters[a]"] == ["one"], query
        assert query["filters[z]"] == ["two"], query
        assert query["api_key"] == ["secret"], query
        assert "redirect=https://example.test/a?x=1" in parsed.query, parsed.query
        assert query["force"] == ["true"], query
        assert "strict=https%3A%2F%2Fstrict.test%2Fa%3Fx%3D1" in parsed.query, parsed.query
        assert "strict=https://free.test/a?x=1" in parsed.query, parsed.query
        headers = {key.lower(): value for key, value in request.header_items()}
        assert headers["x-signature"] == "sig", headers
        assert headers["x-tags"] == "red,blue", headers
        assert headers["x-api-key"] == "secret", headers
        assert headers["cookie"] == "session=session%2Fwith%20space%2Bplus", headers
        return Response()


client = Client("https://api.test", api_key="secret", opener=Opener())
client.send_wire(
    ["active", "pending"],
    "https://example.test/a?x=1",
    "sig",
    filters={"z": "two", "a": "one"},
    x_tags=["red", "blue"],
    session="session/with space+plus",
    force=True,
    strict="https://strict.test/a?x=1",
    free={"strict": "https://free.test/a?x=1"},
)
"#;
