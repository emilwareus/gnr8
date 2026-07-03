//! Integration tests for the `gnr8 compat` binary boundary.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

const GNR8_BIN: &str = env!("CARGO_BIN_EXE_gnr8");

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "gnr8-compat-{label}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn run_gnr8(root: &Path, args: &[String]) -> (bool, String, String) {
    let output = Command::new(GNR8_BIN)
        .args(args)
        .current_dir(root)
        .output()
        .expect("spawn gnr8");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn parse_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or_else(|err| panic!("invalid JSON: {err}\n{stdout}"))
}

fn write_go_sdk(dir: &Path, body: &str) {
    std::fs::create_dir_all(dir).expect("create Go SDK dir");
    std::fs::write(dir.join("go.mod"), "module example.com/sdk\n\ngo 1.23\n")
        .expect("write go.mod");
    std::fs::write(dir.join("sdk.go"), body).expect("write sdk.go");
}

fn write_go_compat_pair(root: &Path) -> (PathBuf, PathBuf) {
    let old = root.join("old-go");
    let new = root.join("new-go");
    write_go_sdk(
        &old,
        r"package sdk

type Book struct{}
type Configuration struct{}
type ApiListBooksRequest struct{}

func NewConfiguration() *Configuration { return nil }
func (r ApiListBooksRequest) PageSize(pageSize any) ApiListBooksRequest { return r }
func (r ApiListBooksRequest) Execute() (*Book, *http.Response, error) { return nil, nil, nil }
",
    );
    write_go_sdk(
        &new,
        r"package sdk

type Book struct{}
type Configuration struct{}
type ApiListBooksRequest struct{}

func NewConfiguration(baseURL string) *Configuration { return nil }
func (r ApiListBooksRequest) PageSize(pageSize int64) ApiListBooksRequest { return r }
func (r ApiListBooksRequest) Execute(ctx context.Context) (*Book, *http.Response, error) {
	return nil, nil, nil
}
",
    );
    (old, new)
}

fn write_typescript_sdk(dir: &Path, model: &str, api: &str) {
    std::fs::create_dir_all(dir).expect("create TypeScript SDK dir");
    std::fs::write(
        dir.join("package.json"),
        r#"{"main":"dist/index.js","types":"dist/index.d.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        dir.join("index.ts"),
        "export * from \"./models\";\nexport * from \"./api\";\n",
    )
    .expect("write index.ts");
    std::fs::write(dir.join("models.ts"), model).expect("write models.ts");
    std::fs::write(dir.join("api.ts"), api).expect("write api.ts");
}

fn write_typescript_compat_pair(root: &Path) -> (PathBuf, PathBuf) {
    let old = root.join("old-ts");
    let new = root.join("new-ts");
    write_typescript_sdk(
        &old,
        "export interface Book {\n  title?: string | null;\n  author: string;\n}\n",
        r"export class DefaultApi {
  async listBooks(): Promise<AxiosResponse<Book>> { return response; }
}
export const DefaultApiFactory = function () {
  return {
    listBooks(): Promise<AxiosResponse<Book>> { return api.listBooks(); },
  };
};
",
    );
    write_typescript_sdk(
        &new,
        "export interface Book {\n  title: string;\n}\n",
        r"export class DefaultApi {
  async listBooks(): Promise<Book> { return book; }
}
export const DefaultApiFactory = function () {
  return {
    listBooks(): Promise<Book> { return api.listBooks(); },
  };
};
",
    );
    (old, new)
}

fn string_array_contains(value: &serde_json::Value, needle: &str) -> bool {
    value
        .as_array()
        .unwrap_or_else(|| panic!("expected array, got {value:?}"))
        .iter()
        .any(|item| item.as_str() == Some(needle))
}

#[test]
fn compat_go_without_contract_preserves_breaking_exit_code() {
    let root = unique_temp_dir("go-no-contract");
    let (old, new) = write_go_compat_pair(&root);

    let (ok, stdout, stderr) = run_gnr8(
        &root,
        &[
            "--json".to_string(),
            "compat".to_string(),
            "go".to_string(),
            "--old".to_string(),
            old.display().to_string(),
            "--new".to_string(),
            new.display().to_string(),
        ],
    );

    assert!(
        !ok,
        "raw breaking diff must exit non-zero\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let report = parse_json(&stdout);
    assert_eq!(report["language"], "go");
    assert_eq!(report["breaking"], true);
    assert!(report["contract_evaluation"].is_null());
    assert!(report["diff"]["exported_method_signature_changes"]
        .as_array()
        .expect("method changes array")
        .iter()
        .any(|change| change["symbol"] == "ApiListBooksRequest.Execute"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_go_contract_allows_approved_drift_and_reports_stale_allowances() {
    let root = unique_temp_dir("go-contract");
    let (old, new) = write_go_compat_pair(&root);
    let contract = root.join("compat.toml");
    std::fs::write(
        &contract,
        r#"[go]
require_exported_types = ["Book"]
allow_exported_function_signature_changes = ["NewConfiguration"]
allow_exported_method_signature_changes = ["ApiListBooksRequest.Execute", "ApiListBooksRequest.PageSize"]
allow_missing_exported_types = ["LegacyBook"]
"#,
    )
    .expect("write contract");

    let (ok, stdout, stderr) = run_gnr8(
        &root,
        &[
            "--json".to_string(),
            "compat".to_string(),
            "go".to_string(),
            "--old".to_string(),
            old.display().to_string(),
            "--new".to_string(),
            new.display().to_string(),
            "--contract".to_string(),
            contract.display().to_string(),
        ],
    );

    assert!(
        ok,
        "approved drift should exit zero\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let report = parse_json(&stdout);
    assert_eq!(report["breaking"], false);
    assert_eq!(report["contract"], contract.display().to_string());
    assert_eq!(report["contract_evaluation"]["breaking"], false);
    assert_eq!(
        report["contract_evaluation"]["unapproved_diff"]["exported_method_signature_changes"]
            .as_array()
            .expect("unapproved method changes array")
            .len(),
        0
    );
    assert!(string_array_contains(
        &report["contract_evaluation"]["stale_allowances"],
        "go.allow_missing_exported_types: LegacyBook"
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_go_contract_missing_required_symbol_fails_even_with_allowed_drift() {
    let root = unique_temp_dir("go-required");
    let (old, new) = write_go_compat_pair(&root);
    let contract = root.join("compat.toml");
    std::fs::write(
        &contract,
        r#"[go]
require_exported_functions = ["MissingClient"]
allow_exported_function_signature_changes = ["NewConfiguration"]
allow_exported_method_signature_changes = ["ApiListBooksRequest.Execute", "ApiListBooksRequest.PageSize"]
"#,
    )
    .expect("write contract");

    let (ok, stdout, stderr) = run_gnr8(
        &root,
        &[
            "--json".to_string(),
            "compat".to_string(),
            "go".to_string(),
            "--old".to_string(),
            old.display().to_string(),
            "--new".to_string(),
            new.display().to_string(),
            "--contract".to_string(),
            contract.display().to_string(),
        ],
    );

    assert!(
        !ok,
        "missing required symbol must exit non-zero\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let report = parse_json(&stdout);
    assert_eq!(report["breaking"], true);
    assert!(string_array_contains(
        &report["contract_evaluation"]["missing_required"],
        "go.require_exported_functions: MissingClient"
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_go_suggest_json_includes_high_confidence_snippets() {
    let root = unique_temp_dir("go-suggest");
    let (old, new) = write_go_compat_pair(&root);

    let (ok, stdout, stderr) = run_gnr8(
        &root,
        &[
            "--json".to_string(),
            "compat".to_string(),
            "go".to_string(),
            "--old".to_string(),
            old.display().to_string(),
            "--new".to_string(),
            new.display().to_string(),
            "--suggest".to_string(),
        ],
    );

    assert!(
        !ok,
        "suggest mode should preserve breaking exit code\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let report = parse_json(&stdout);
    assert!(report["suggestions"]
        .as_array()
        .expect("suggestions array")
        .iter()
        .any(|suggestion| suggestion
            .as_str()
            .is_some_and(|text| text.contains("GoExecuteCompatibility::preserve_legacy()"))));
    assert!(report["suggestions"]
        .as_array()
        .expect("suggestions array")
        .iter()
        .any(|suggestion| suggestion
            .as_str()
            .is_some_and(|text| text.contains("GoQuerySetterArgumentPolicy::typed()"))));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compat_typescript_contract_allows_approved_drift_and_suggests_json() {
    let root = unique_temp_dir("ts-contract");
    let (old, new) = write_typescript_compat_pair(&root);
    let contract = root.join("compat.toml");
    std::fs::write(
        &contract,
        r#"[typescript]
require_root_exports = ["Book", "DefaultApi"]
allow_missing_interface_properties = ["Book.author"]
allow_interface_property_changes = ["Book.title"]
allow_operation_return_type_changes = ["DefaultApi.listBooks", "DefaultApiFactory.listBooks"]
allow_operation_signature_changes = ["DefaultApi.listBooks", "DefaultApiFactory.listBooks"]
allow_missing_request_aliases = ["CreateBookRequest"]
"#,
    )
    .expect("write contract");

    let (ok, stdout, stderr) = run_gnr8(
        &root,
        &[
            "--json".to_string(),
            "compat".to_string(),
            "typescript".to_string(),
            "--old".to_string(),
            old.display().to_string(),
            "--new".to_string(),
            new.display().to_string(),
            "--contract".to_string(),
            contract.display().to_string(),
            "--suggest".to_string(),
        ],
    );

    assert!(
        ok,
        "approved TypeScript drift should exit zero\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    let report = parse_json(&stdout);
    assert_eq!(report["language"], "typescript");
    assert_eq!(report["breaking"], false);
    assert_eq!(report["contract_evaluation"]["breaking"], false);
    assert!(string_array_contains(
        &report["contract_evaluation"]["stale_allowances"],
        "typescript.allow_missing_request_aliases: CreateBookRequest"
    ));
    assert_eq!(
        report["contract_evaluation"]["unapproved_diff"]["interface_property_changes"]
            .as_array()
            .expect("unapproved interface property changes")
            .len(),
        0
    );
    assert_eq!(
        report["suggestions"]
            .as_array()
            .expect("suggestions array")
            .len(),
        0,
        "suggestions should be generated from unapproved drift when a contract is present"
    );

    let _ = std::fs::remove_dir_all(root);
}
