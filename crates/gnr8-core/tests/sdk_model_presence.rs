//! A generated model's field presence is answered from the direction its schema is reached from.
//!
//! "May a caller leave this key out?" and "may the server?" are the same question asked on opposite
//! sides of an exchange, and the graph answers them with different facts: `field.required` is what the
//! server's own validation rejects an INBOUND payload for lacking, `field.optional` is what the
//! serializer may leave out of a payload it WRITES. A model reads the validation rules exactly where it
//! is inbound and only inbound; everywhere else the presence axis stands. See
//! `graph::direction::SchemaDirections::model_field_is_optional`.
//!
//! One graph puts the same four fields in each of the four positions:
//!
//! | schema       | reached from        |
//! |--------------|---------------------|
//! | `CreateInput`| a request body      |
//! | `Payload`    | a response body     |
//! | `Shared`     | both                |
//! | `Unwired`    | no operation        |
//!
//! Two things vary across the expectations below, not one. Where the schema sits decides the answer,
//! and the target decides how much of that answer it can spell: `?:` and `= None` mean "may be left
//! out" and take it whole, while Go's `,omitempty` means "drop the zero value" and can only ever have
//! the option taken away — which is why [`GO_REQUEST_ONLY`] exists beside [`REQUEST_ONLY`] rather than
//! reusing it.
//!
//! The Go SDK path pipes each file through `gofmt`, so this test needs the Go toolchain — the same
//! prerequisite `snapshot_sdk` already carries.

// Tests legitimately use unwrap/expect/panic (rust-best-practices skill ch.4).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use gnr8::graph::ApiGraph;
use gnr8::sdk::layout::SdkFileLayout;
use gnr8::sdk::model_style::PyModelStyle;

/// The four axis combinations a Go struct tag can produce, in one object body. `validated` is the pair
/// the source can state and the two artifacts used to disagree about: `binding:"required"` alongside
/// `json:",omitempty"`, which is ordinary Go.
const FIELDS: &str = r#"[
  { "json_name": "loose", "required": false, "optional": true, "nullable": false,
    "schema": { "type": "primitive", "of": { "prim": "string" } } },
  { "json_name": "mandatory", "required": true, "optional": false, "nullable": false,
    "schema": { "type": "primitive", "of": { "prim": "string" } } },
  { "json_name": "plain", "required": false, "optional": false, "nullable": false,
    "schema": { "type": "primitive", "of": { "prim": "string" } } },
  { "json_name": "validated", "required": true, "optional": true, "nullable": false,
    "schema": { "type": "primitive", "of": { "prim": "string" } } }
]"#;

fn graph() -> ApiGraph {
    let json = format!(
        r#"{{
          "module": "presence.test",
          "operations": [
            {{
              "id": "createItem", "method": "POST", "path": "/items", "handler": "createItem",
              "params": [],
              "request_body": {{ "ref_id": "dto.CreateInput" }},
              "request_body_content_type": "application/json",
              "responses": [
                {{ "status": 200, "body": {{ "ref_id": "dto.Payload" }},
                   "content_types": ["application/json"] }}
              ],
              "provenance": {{ "file": "items.go", "start_line": 1, "end_line": 1 }}
            }},
            {{
              "id": "echoShared", "method": "POST", "path": "/shared", "handler": "echoShared",
              "params": [],
              "request_body": {{ "ref_id": "dto.Shared" }},
              "request_body_content_type": "application/json",
              "responses": [
                {{ "status": 200, "body": {{ "ref_id": "dto.Shared" }},
                   "content_types": ["application/json"] }}
              ],
              "provenance": {{ "file": "shared.go", "start_line": 1, "end_line": 1 }}
            }}
          ],
          "schemas": [
            {{ "id": "dto.CreateInput", "name": "CreateInput",
               "body": {{ "type": "object", "of": {FIELDS} }},
               "provenance": {{ "file": "dto.go", "start_line": 1, "end_line": 1 }} }},
            {{ "id": "dto.Payload", "name": "Payload",
               "body": {{ "type": "object", "of": {FIELDS} }},
               "provenance": {{ "file": "dto.go", "start_line": 2, "end_line": 2 }} }},
            {{ "id": "dto.Shared", "name": "Shared",
               "body": {{ "type": "object", "of": {FIELDS} }},
               "provenance": {{ "file": "dto.go", "start_line": 3, "end_line": 3 }} }},
            {{ "id": "dto.Unwired", "name": "Unwired",
               "body": {{ "type": "object", "of": {FIELDS} }},
               "provenance": {{ "file": "dto.go", "start_line": 4, "end_line": 4 }} }}
          ],
          "diagnostics": [],
          "base_path": "/api",
          "title": "Presence",
          "security": []
        }}"#
    );
    serde_json::from_str(&json).expect("presence graph must deserialize")
}

/// Whether each field's key may be left out of the model, for a schema reached ONLY from a request.
///
/// The server's validation is the whole of what an omission is accepted or rejected against here:
/// an omission option governs marshalling and the server never marshals a request DTO, so the presence
/// axis describes nothing about what a client may send.
const REQUEST_ONLY: [(&str, bool); 4] = [
    ("loose", true),
    ("mandatory", false),
    ("plain", true),
    // The reported defect: a caller could omit a key the server rejects the request for lacking.
    ("validated", false),
];

/// Whether each field's key may be left out of the model everywhere else — reached from a response,
/// from both directions, or from no operation at all.
///
/// The model is (or may be) the decode side, where the key's absence is the server's choice and not
/// the caller's, and demanding it breaks a payload the server is entitled to send.
const PRESENCE_AXIS: [(&str, bool); 4] = [
    ("loose", true),
    ("mandatory", false),
    ("plain", false),
    ("validated", true),
];

/// Whether each field carries `,omitempty` on a schema reached ONLY from a request.
///
/// This is NOT [`REQUEST_ONLY`], and the one row that differs is the point: `plain` may be left out of
/// a request as far as the server is concerned, but `,omitempty` does not spell "may be left out" — it
/// drops the zero value, so writing it where the source did not would cost the caller `"plain": ""`
/// without buying absence. The direction may only take the option away, so Go's request answer is the
/// source's own option narrowed by [`REQUEST_ONLY`], and only `validated` loses it.
const GO_REQUEST_ONLY: [(&str, bool); 4] = [
    ("loose", true),
    ("mandatory", false),
    ("plain", false),
    ("validated", false),
];

/// The lines of one emitted model declaration: everything after `header` up to the next line at column
/// zero, which is what closes a Go struct, a TypeScript interface, and a Python class body alike.
///
/// Lines are trimmed and their inner whitespace runs collapsed, so a formatter's column alignment never
/// decides an assertion. Bounding on indentation rather than on a per-language terminator means there
/// is no terminator to get wrong: a mismatched one used to silently widen the slice to the rest of the
/// bundle and let an assertion pass on a line some other model emitted.
fn declaration_lines(out: &str, header: &str) -> Vec<String> {
    let start = out
        .find(header)
        .unwrap_or_else(|| panic!("{header:?} missing from:\n{out}"));
    let rest = &out[start + header.len()..];
    rest.lines()
        // A blank line does not end a declaration (Python puts one before `@classmethod`); a non-blank
        // line that starts at column zero does.
        .take_while(|line| line.trim().is_empty() || line.starts_with([' ', '\t']))
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect()
}

fn assert_declares(lines: &[String], expected: &str, model: &str) {
    assert!(
        lines.iter().any(|line| line == expected),
        "{model} must declare `{expected}`, got:\n{}",
        lines.join("\n")
    );
}

#[test]
fn go_models_omitempty_follows_the_direction_the_schema_is_reached_from() {
    let out = gnr8::gosdk::generate(&graph(), "presence", "/api")
        .expect("Go SDK generation must succeed (requires gofmt)");
    for (model, table) in [
        ("CreateInput", GO_REQUEST_ONLY),
        ("Payload", PRESENCE_AXIS),
        ("Shared", PRESENCE_AXIS),
        ("Unwired", PRESENCE_AXIS),
    ] {
        let lines = declaration_lines(&out, &format!("type {model} struct {{"));
        for (field, omit_empty) in table {
            let exported = format!("{}{}", field[..1].to_uppercase(), &field[1..]);
            let tag = if omit_empty {
                format!("`json:\"{field},omitempty\"`")
            } else {
                format!("`json:\"{field}\"`")
            };
            assert_declares(&lines, &format!("{exported} string {tag}"), model);
        }
    }
}

#[test]
fn typescript_interfaces_mark_optional_from_the_direction_the_schema_is_reached_from() {
    let out = gnr8::tssdk::generate(&graph(), "presence", "/api")
        .expect("TypeScript SDK generation must succeed");
    for (model, table) in [
        ("CreateInput", REQUEST_ONLY),
        ("Payload", PRESENCE_AXIS),
        ("Shared", PRESENCE_AXIS),
        ("Unwired", PRESENCE_AXIS),
    ] {
        let lines = declaration_lines(&out, &format!("export interface {model} {{"));
        for (field, omittable) in table {
            let mark = if omittable { "?" } else { "" };
            assert_declares(&lines, &format!("{field}{mark}: string;"), model);
        }
    }
}

#[test]
fn pydantic_models_default_from_the_direction_the_schema_is_reached_from() {
    let out = gnr8::pysdk::generate(&graph(), "presence", "/api")
        .expect("Python SDK generation must succeed");
    for (model, table) in [
        ("CreateInput", REQUEST_ONLY),
        ("Payload", PRESENCE_AXIS),
        ("Shared", PRESENCE_AXIS),
        ("Unwired", PRESENCE_AXIS),
    ] {
        let lines = declaration_lines(&out, &format!("class {model}(BaseModel):"));
        for (field, omittable) in table {
            let declaration = if omittable {
                format!("{field}: Optional[str] = Field(default=None)")
            } else {
                format!("{field}: str")
            };
            assert_declares(&lines, &declaration, model);
        }
    }
}

#[test]
fn dataclass_models_and_their_decoders_agree_on_the_direction() {
    let out = gnr8::pysdk::generate_with_options(
        &graph(),
        "presence",
        "/api",
        &SdkFileLayout::compact(),
        PyModelStyle::Dataclass,
    )
    .expect("Python dataclass SDK generation must succeed");
    for (model, table) in [
        ("CreateInput", REQUEST_ONLY),
        ("Payload", PRESENCE_AXIS),
        ("Shared", PRESENCE_AXIS),
        ("Unwired", PRESENCE_AXIS),
    ] {
        let lines = declaration_lines(&out, &format!("class {model}:"));
        for (field, omittable) in table {
            let declaration = if omittable {
                format!("{field}: Optional[str] = None")
            } else {
                format!("{field}: str")
            };
            assert_declares(&lines, &declaration, model);
        }
    }
    // A `@dataclass` whose declaration and whose `from_dict` disagreed would raise at construction or
    // decode, so the decoder has to move with the declaration: a required attribute is bound from the
    // key directly, and an omittable one only when the key is there.
    let decoder = declaration_lines(&out, "class CreateInput:");
    // The slice covers this one class and stops: over-running into the next would bring three more
    // decoders with it, and the assertions below could then pass on a line CreateInput never emitted.
    assert_eq!(
        decoder
            .iter()
            .filter(|line| line.starts_with("validated="))
            .count(),
        1,
        "the decoder slice must cover exactly one class:\n{}",
        decoder.join("\n")
    );
    assert!(
        decoder
            .iter()
            .any(|line| line == "validated=_data[\"validated\"],"),
        "a request-validated field must decode from the key directly:\n{}",
        decoder.join("\n")
    );
    assert!(
        decoder
            .iter()
            .any(|line| line.starts_with("plain=(") && line.contains("if \"plain\" in _data")),
        "a field no rule validates must decode only when the key is present:\n{}",
        decoder.join("\n")
    );
}

/// Every SDK emits models twice — one file per schema under a split layout, all of them in one file
/// under the compact one — and the two paths are separate functions. A rule wired into one and not the
/// other would pass every test above, so pin the split path on the case that matters.
#[test]
fn a_split_layout_reaches_the_same_answer_as_a_compact_one() {
    let split = SdkFileLayout::split();
    let outputs = [
        (
            "Go",
            gnr8::gosdk::generate_with_layout(&graph(), "presence", "/api", &split)
                .expect("Go split SDK generation must succeed (requires gofmt)"),
            "type {model} struct {",
            "Validated string `json:\"validated\"`",
            "Loose string `json:\"loose,omitempty\"`",
        ),
        (
            "TypeScript",
            gnr8::tssdk::generate_with_layout(&graph(), "presence", "/api", &split)
                .expect("TypeScript split SDK generation must succeed"),
            "export interface {model} {",
            "validated: string;",
            "loose?: string;",
        ),
        (
            "Python",
            gnr8::pysdk::generate_with_options(
                &graph(),
                "presence",
                "/api",
                &split,
                PyModelStyle::Pydantic,
            )
            .expect("Python split SDK generation must succeed"),
            "class {model}(BaseModel):",
            "validated: str",
            "loose: Optional[str] = Field(default=None)",
        ),
    ];
    for (language, out, header, required, omittable) in outputs {
        // The request-only model demands the validated key and lets the unvalidated one go...
        let request_only = declaration_lines(&out, &header.replace("{model}", "CreateInput"));
        assert_declares(&request_only, required, &format!("{language} CreateInput"));
        assert_declares(&request_only, omittable, &format!("{language} CreateInput"));
        // ...and the response-only model, split into its own file, still reads the presence axis.
        let response_only = declaration_lines(&out, &header.replace("{model}", "Payload"));
        assert_declares(&response_only, omittable, &format!("{language} Payload"));
    }
}

/// A multipart request body whose schema is ALSO a response body, so the walk reports both directions
/// for it while the inline argument still occupies only the request position.
fn multipart_both_ways_graph() -> ApiGraph {
    let json = format!(
        r#"{{
          "module": "multipart.test",
          "operations": [
            {{
              "id": "upload", "method": "POST", "path": "/uploads", "handler": "upload",
              "params": [],
              "request_body": {{ "ref_id": "dto.Upload" }},
              "request_body_content_type": "multipart/form-data",
              "responses": [
                {{ "status": 200, "body": {{ "ref_id": "dto.Upload" }},
                   "content_types": ["application/json"] }}
              ],
              "provenance": {{ "file": "upload.go", "start_line": 1, "end_line": 1 }}
            }}
          ],
          "schemas": [
            {{ "id": "dto.Upload", "name": "Upload",
               "body": {{ "type": "object", "of": {FIELDS} }},
               "provenance": {{ "file": "dto.go", "start_line": 1, "end_line": 1 }} }}
          ],
          "diagnostics": [],
          "base_path": "/api",
          "title": "Multipart",
          "security": []
        }}"#
    );
    serde_json::from_str(&json).expect("multipart graph must deserialize")
}

/// A multipart body's inline argument type takes the REQUEST answer even when the schema it comes from
/// is reached from both directions, and the named interface for that same schema does not.
///
/// The two are different positions, not two answers to one question: the argument is only ever built
/// to be sent, so narrowing it to what the server accepts costs the caller nothing; `models.Upload`
/// has to survive a decode as well, so it keeps the answer that cannot break one.
#[test]
fn a_multipart_argument_is_a_request_even_when_its_schema_is_also_a_response() {
    let out = gnr8::tssdk::generate(&multipart_both_ways_graph(), "multipart", "/api")
        .expect("TypeScript multipart SDK generation must succeed");

    // The inline argument: `validated` is demanded, `plain` and `loose` are not.
    let argument = out
        .split("body: {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or_else(|| panic!("no inline multipart argument in:\n{out}"));
    for (field, omittable) in REQUEST_ONLY {
        let mark = if omittable { "?" } else { "" };
        assert!(
            argument.contains(&format!("{field}{mark}: ")),
            "the multipart argument must declare `{field}{mark}:`, got:\n{argument}"
        );
    }

    // The named interface for the same schema is reached from both directions and keeps the presence
    // axis, so `validated` is marked `?:` there while the argument above demands it.
    let interface = declaration_lines(&out, "export interface Upload {");
    for (field, omittable) in PRESENCE_AXIS {
        let mark = if omittable { "?" } else { "" };
        assert_declares(&interface, &format!("{field}{mark}: string;"), "Upload");
    }
}
