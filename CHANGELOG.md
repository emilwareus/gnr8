# Changelog

Notable changes to `gnr8`. Entries under **Unreleased** land in the next release; the
[release runbook](docs/RELEASE.md) chooses `patch` or `minor` based on whether this section
contains a **Breaking** heading.

Cargo treats `0.x.y → 0.x.(y+1)` as a compatible upgrade, so any release with breaking changes
must move the minor version.

## Unreleased

### Breaking

- **Presence and nullability are now modeled independently for input and output payloads.** The
  extraction contract records serializer omission, deserializer absence/null acceptance, serializer
  null emission, and validator presence/null rejection as separate facts. OpenAPI, Go, Python, and
  TypeScript generation all derive their field shape from the payload direction instead of treating
  null as permission to omit a key.

  Consequently, a response field whose serializer always writes the key is required even when its
  value can be null (`field: T | null`, not `field?: T | null`). A field with a JSON omission option
  is optional on output and, for ordinary pointer/slice/map values, non-null when present. Required
  request slices reject null; `json.RawMessage` keeps its distinct ability to hold literal JSON null.

  When one source type is used in both directions and those contracts differ, generation now emits
  distinct `TypeInput` and `TypeOutput` schemas/models and rewrites transitive references. Non-HTTP
  payloads can declare their direction with `register_input_schema` or `register_output_schema`, and
  checked `force_nullable` / `force_non_nullable` overrides take an explicit `SchemaUse`.

  Go SDK optional value fields now use pointers with `omitempty`, preserving both absence and an
  explicit zero value; required nullable value fields remain pointers without an omission tag.
  Python `to_dict()` retains null for required-nullable keys so its own output remains decodable.

  **Upgrade note for 0.6.x consumers:** regenerate committed SDKs and review constructor call sites,
  component names, and schema assertions. In particular, nullable response fields that 0.6.1 made
  omittable become required again, while omitted ordinary container fields no longer advertise null.

### Fixed

- **Upgrading the Go toolchain no longer leaves a stale `goextract` binary behind.** The compiled
  helper was cached under a key covering only its own source, so a user who upgraded Go kept running
  the binary their PREVIOUS toolchain built. `go/types` admits only the language version the
  application was built with, so that binary then reported every dependency file gated on the new
  release as a `source.load.failed` load error — on Go 1.27 a stock Gin project fails this way through
  `golang.org/x/text`, which ships `//go:build go1.27` tables. Nothing in the project changed, and no
  amount of re-running fixed it; only clearing a temp directory nobody knows about did.

  The cache key now covers the toolchain identity as well as the source. This is the same fact
  `sdk::builtins::go_toolchain_identity` already folds into the extracted-facts cache key, for the
  same reason: the selected toolchain decides which build constraints pick which files and what
  stdlib type information the extractor sees, which makes it an extraction input like any source
  file. The binary cache was the one place that had not accounted for it.

  Both now read that identity from the **analyzed module**, and the `goextract` build is pinned to it.
  `go/packages` runs `go list` inside the target, so a service whose `go.mod` asks for a newer Go than
  the machine's `PATH` carries is type-checked by that newer release — and building the helper from
  its own directory, where `goextract/go.mod` selects the toolchain, produced exactly the same
  too-old-`go/types` failure by a second route. The pin preserves the caller's selection policy:
  `auto` may resolve upward, `path` remains download-free, and `local` or an exact selection remains
  fixed.

## 0.6.1 — 2026-08-20

### Fixed

- **A generated model no longer demands a key whose value it lets you leave null.** 0.6.0 made a bare
  Go pointer and a nil slice/map/interface what they are on the `encoding/json` wire — a key the
  serializer always writes, holding a value that may be `null` — and generated models read the presence
  axis straight, so every one of those fields became mandatory at construction while still being hinted
  `Optional[...]` / `T | null`. `PySdk` emitted `user_uuid: Optional[str] = Field(..., alias="userUuid")`
  and `TsSdk` emitted `userUuid: string | null`, where 0.5.3 had emitted a default and a `?`. Callers
  who never wrote `userUuid=None, namespaceId=None, uniqueKey=None` got a `ValidationError` counting
  every null-valued key they had left implicit.

  The demand bought the reading side nothing. A nullable key's hint carries the absent case either way
  and the value a reader ends up with is `None`/`null` either way, so presence changed nothing about
  reading it and only added a value the writing side had to spell — one `PySdk`'s own `to_dict` then
  dropped, since `exclude_none` omits exactly the null-valued keys such a declaration demanded back. A
  model could not decode its own output: `DtoEvent.from_dict(event.to_dict())` raised.

  A nullable field is now omittable in a generated model wherever no validation rule demands its key,
  which is every position the model is not the inbound side of — reached from a response, or from no
  operation at all — and the both-directions position for a field no `binding:`/`validate:` rule
  covers. Where such a rule does cover it, the key stays demanded in every position the model is
  inbound from: that demand is a fact about what the server rejects a request for lacking, not about
  what the model can express, and dropping it there would reopen the request-side defect 0.6.0 fixed.

  **This changes `PySdk` and `TsSdk` output for Go sources**, and restores 0.5.3's construction
  behavior for nullable fields while keeping 0.6.0's for non-nullable ones: a non-nullable key the
  server writes on every response stays required
  (`event_type_identifier: str = Field(..., alias="eventTypeIdentifier")`), and a nullable one regains
  its default (`user_uuid: Optional[str] = Field(default=None, alias="userUuid")`,
  `userUuid?: string | null`). In this repository that reaches three committed models —
  `ListBooksResponse.nextCursor` in the FastAPI and NestJS examples and `OrderConfirmation.message`
  in the Flask one. **`GoSdk` output and emitted documents are unchanged.** Go has no spelling for "may
  be left out" and the direction may only ever take `,omitempty` away, which this rule never does; and
  `required` describes the payload, where a bare pointer's key genuinely is written every time.

## 0.6.0 — 2026-08-20

### Fixed

- **A `oneof` on a bound Go parameter now lands on the value its scope names.** The parameter reader
  took the first `oneof=` in a `binding:`/`validate:` tag regardless of scope and placed it on the
  array element, which happened to be right for a `[]string` and destroyed a map: `Filters
  map[string]string` tagged `binding:"dive,oneof=red green"` was published as a bare string enum
  rather than a map, and writing the rule without the `dive` did not avoid it, because there was no
  destination for an enum on a map at either scope. It was the last reader of these tags that did not
  go through the scope-aware tokenizer added in 0.5.3 (#55).

  Each scope now has exactly one destination. Field scope lands on a scalar parameter's own schema,
  and on a container it lowers to what the container holds — a `facts.Type` array or map has no room
  for an enum beside it, so the members can only be describing the values. `dive` lands on an array's
  element or a map's **values**, leaving the key the plain string OpenAPI requires. A scope that
  reaches no schema is dropped in silence, as it already is on a schema field: that covers a `dive` on
  a scalar, and also `keys`…`endkeys`, since an OpenAPI object key is always an unconstrained string
  and the lowering rejects any map key that is not one — applying an enum there would abort a whole
  document over a single well-formed struct tag.

  **Two rules landing on the same value are now a `request.parameter.ambiguous` ERROR** naming each
  rule with the tag key that carries it, and neither is applied. That covers
  `binding:"oneof=a b,dive,oneof=c d"` on a `[]string`, and it also covers the
  same enum restated under a second tag key — `binding:` and `validate:` are read as peers now, where
  `binding:` used to win in silence, and the `enums:`/`enum:` tag is a peer of both rather than a
  fallback consulted only when no `oneof` was found. Restating a fact identically is reported too:
  choosing between two statements would be a precedence rule, and gnr8 has no winner to pick.

  **This changes emitted documents** for map parameters (previously replaced by an enum, now a map
  whose values carry it) and for any parameter whose enum was stated twice (previously the first
  spelling won, now no enum is applied and an ERROR is raised). The ERROR is reported, not fatal —
  deny `request.parameter.ambiguous` with a `DiagnosticPolicy` to fail the build on it.

- **Generated Python dataclass models no longer crash on a `null` they document.** `from_dict`
  decoded every non-optional field unconditionally, so a required-but-nullable field whose decode
  dereferences the value — a list of nested models, or a nested model — raised
  `TypeError: 'NoneType' object is not iterable` (or failed inside `from_dict(None)`) when the
  server sent the `null` the document permits. The model's own hint said `Optional[...]`, so the SDK
  promised a tolerance it did not implement.

  The decode is now guarded the way the optional branch already was. The guard tests the **value**,
  not presence: a missing key still raises `KeyError`, because that is a real protocol error for a
  field the document requires. A passthrough decode is left alone — it already yields `None` — so
  nullable scalars gain no noise.

  Pydantic models (the default) and the TypeScript SDK were already correct. This defect predates
  the field-derivation fix below, but that fix makes required-but-nullable the common shape for
  every bare slice, map, and interface, so it is what turns a latent crash into a likely one.

- **Go field presence and nullability now say what `encoding/json` actually does.** Both axes were
  derived partly from the declared type, which is evidence for neither. A field was marked optional
  when it was a pointer and nullable only when it was a pointer, so three shapes came out wrong:

  - A **nil slice, map, or interface** marshals to `null`, but only a pointer was published as
    nullable. A generated Python model hinted `list[X]` with no `| None`, and a documented response
    containing `"logs": null` was rejected by `model_validate` before user code saw it. This is the
    same failure as the `,omitzero` defect fixed in 0.5.2, reached through the value axis instead of
    the presence axis.
  - A **bare pointer** (`*T json:"k"`, no omission option) keeps its key — `encoding/json` writes
    `"k":null`. It was published as optional, so every SDK let the key be absent when it never is.
  - **`,omitempty` is a no-op on a struct, a `time.Time`, and a non-zero-length array.** The option
    omits only `false`, `0`, `""`, a nil pointer/interface, and a zero-length array/slice/map, so on
    those types the key is always written. The field was published as optional anyway. gnr8 now
    publishes it as always-present and raises `schema.omit_option.ineffective` (`INFO`) naming the
    field and pointing at `,omitzero`, which does omit the zero value of any type.

  On the `encoding/json` wire each axis now has exactly one source: presence from the tag's omission
  option, nullability from the declared type. A `form:`-tagged field keeps its own presence rule —
  that binder is not `encoding/json` and this change does not speak for it — but loses the value
  axis, because a multipart part is present or absent and there is no `null` to write.

  Nullability is applied whatever omission option the field carries, and that takes the **inbound**
  direction to justify rather than "what the marshaller writes": an omission-tagged field never
  marshals `null`, because a nil value is dropped before it can be written, but `json.Unmarshal`
  accepts an explicit `null` into a pointer, slice, map, or interface whatever the tag says. So the
  axis is exactly right for a request body and wider than a response body can produce. That is the
  safe side of the failure this release fixes — tolerating a `null` that never arrives costs
  nothing, while rejecting one that does breaks user code — but narrowing it means knowing which
  direction a schema is reached from. The entry below establishes that direction where the document
  is written; the extractor that decides this axis is upstream of it and still cannot see it, so
  narrowing the value axis by direction is a separate change this release does not make.

  **This changes emitted documents and all three generated SDKs.** Every slice, map, and interface
  field gains `"null"` in its document type and `| None` / `| null` in generated Python and
  TypeScript models — nullability comes from the declared type alone, so this reaches the field
  whether it carries `,omitempty`, `,omitzero`, or no option at all. The Go SDK is unchanged by that
  half, since a nil slice already round-trips `null`. Bare pointer fields and ineffectively-tagged
  struct/`time.Time` fields stop being optional everywhere, which does reach the Go SDK: `*T
  json:"k"` now emits `json:"k"` where it previously emitted `json:"k,omitempty"`. All of these are
  corrections, but a client generated from the new document accepts responses the old one rejected
  and requires keys the old one did not, so review the regenerated output before publishing.

  The document and the SDKs used to answer "must this field be present?" from different axes
  (`required` from `binding`/`validate`, presence from the `json` tag). That was a
  request-versus-response direction question rather than one about what the marshaller writes, and
  the entry below answers it.

- **A response schema's `required` array is no longer derived from request validation tags.**
  `required` was always the set of fields carrying a `binding:`/`validate:` `required` rule — a fact
  about what the server rejects a *request* for lacking. Nothing in a validation tag describes a
  response, so on a response schema that array carried almost no information: in this repo's own
  fixture, `ListGoalsOutput` published no `required` at all, though three of its four keys are
  written on every response and only `nextCursor` is genuinely omittable. The answer was already on
  the graph one field over — the presence axis the entry above made exact — and unused.

  `required` is now answered from the direction the schema is reached from. The lowering walks the
  graph from each operation's request body and parameters on one side and its response bodies on the
  other, following `$ref`s so a nested type is on whichever side the type carrying it is on:

  - reached from **requests only** → the validation rules, unchanged;
  - reached from **responses only** → every field the serializer writes unconditionally;
  - reached from **both** → only the fields that satisfy both, because one component has to describe
    both payloads and can promise only what holds in each;
  - reached from **no operation** → the validation rules, there being no position to read it from.

  This is one answer per position, not a preference between two candidates, so there is no setting
  for it and no precedence to configure. A struct shared between a request body and a response body
  — `Publisher` in `examples/bookstore`, reached from `Book` and from `CreateBookRequest` — gets the
  narrower answer; splitting it into a type per direction is what makes both questions separately
  answerable.

  **This changes emitted documents for Go sources.** Response schemas gain the keys they always
  contain (`GoalResponse` gains `analyticsQuery` and `createdAt`; `ListGoalsOutput` gains `goals`,
  `pageSize`, and `total`), and a response field that carried a stray `binding:"required"` next to an
  omission option leaves the array — the case where the document demanded a key the SDKs let you
  omit. Python, TypeScript, and OpenAPI-imported sources state presence once and record it on both
  axes, so their documents are byte-identical either way. Generated SDKs are untouched by this
  entry — they read the presence axis, which has not moved here; the entry below moves them onto the
  same walk.

- **`ApiOverrides::force_required` / `force_optional` now state presence instead of correcting one
  axis of it.** Both wrote the graph's `required` field and nothing else. That was already half a
  correction — generated models read the presence axis, so no Go, Python, or TypeScript model had
  ever seen either override, against `ApiOverrides`' own promise that "OpenAPI and every SDK target
  read the same corrected API facts". The entry above would have made it less than half:
  on a schema reached only from responses the document reads the presence axis too, so both
  overrides would have become silent no-ops there — `force_required("Book", "subtitle")` against
  `examples/bookstore`, where `Book` is only ever a response body, would have left `Book.required`
  untouched and reported success.

  Each override now states the fact outright — `force_required` means "this key is always present",
  `force_optional` means "this key may be absent" — and writes both axes, which is how every non-Go
  source already records presence. One statement, the same answer in every direction and in every
  artifact, with nothing left to be dropped on the side that was not written.

  **This changes generated SDKs for pipelines that use either override.** A `force_optional` field
  becomes omittable in the generated models (`,omitempty` in Go, `?` in TypeScript, a default in
  Python) and a `force_required` field stops being omittable. The one model-adjacent surface that
  already read both axes — a TypeScript multipart body's inline part type — is unchanged in meaning,
  since the two axes now agree by construction. Emitted documents change only for schemas reached
  from responses, where the overrides now take effect rather than being discarded.

- **A generated request model no longer lets a caller omit a key the server rejects the request for
  lacking.** `GoSdk`, `PySdk`, and `TsSdk` each read the presence axis unconditionally, so a request
  DTO written the ordinary Go way — a field tagged `json:"name,omitempty"` alongside
  `binding:"required"` — emitted `json:"name,omitempty"`, `name: Optional[str] = None`, and
  `name?: string`. The document said the key was required and every SDK said it was not; in Go the
  caller did not even have to try, since setting the field to its zero value explicitly still sent
  nothing. This is the mirror of the entry above: that one was the document being right for requests
  and wrong for responses, this one is the SDK being right for responses and wrong for requests, and
  both came of answering a directional question with a non-directional fact.

  Models now read the same walk the document reads, which moved out of the lowering so there is one
  implementation of it rather than two:

  - a schema reached from **requests only** → the validation rules. An omission option governs
    marshalling and a server unmarshals a request DTO rather than marshalling one, so the tag
    describes nothing about what a client may leave out; the `binding:`/`validate:` rules state
    exactly what the server demands, and they are the only fact in the source that does.
  - a schema reached from **responses only, from both, or from no operation** → the presence axis,
    unchanged. There the model is, or may be, the decode side, and demanding a key the serializer may
    drop fails on a payload the server is entitled to send.

  The **both** arm is a deliberate choice rather than the document's rule carried over. A document
  publishes the weakest true claim, which is always safe; a model *behaves*, and its optionality is a
  permission on the way out and a tolerance on the way in, so weakest is not safest. Where one type
  carries both contracts and a field is validated *and* omittable, no marking is safe, and the model
  keeps the response answer: the residual failure is then a request the caller can see rejected and
  fix by passing the value, rather than a legal response the SDK cannot decode — the over-required
  response model the presence-axis entry above exists to prevent. Applying the document's
  intersection instead would also have marked optional every field of a both-ways type that carries
  no validation rule at all, which is most of them.

  **In Go the direction may only take an omission option away, never add one.** `?:` and `= None` say
  "the caller may leave this key out" and nothing else, so they read the direction straight.
  `encoding/json` has no such spelling: `,omitempty` says "drop this key when the value is the zero
  value", which only coincides with may-omit where the source already chose it. Writing it where the
  source did not would cost a caller `"price": 0` and `"tags": []` without buying absence — Go needs a
  pointer for that, and gnr8 spends pointers on the nullable axis — and on a struct, a `time.Time`, or
  a non-zero-length array it does nothing at all, which is the tag gnr8 reads in user source as
  `schema.omit_option.ineffective`. So the Go tag omits what the source's own tag omits, narrowed by
  the direction: a `binding:"required"` field tagged `,omitempty` loses the option, and nothing gains
  one.

  **This changes generated SDKs for Go sources**, in one direction for `GoSdk` and both for the other
  two. A validated field carrying an omission option stops being omittable everywhere — a compile
  error for a Python or TypeScript caller who was omitting it, where the call it replaces was a 4xx.
  For `PySdk` and `TsSdk` a request-only field that carries no validation rule additionally becomes
  omittable, matching what the document already published; `GoSdk` leaves those alone for the reason
  above, so a request-only schema's Go model is exactly what it was and its Python and TypeScript
  twins are laxer than before. One shape needs a closer look before publishing:
  `PySdk::dataclasses()` orders defaulted fields last, so a field changing sides moves in the
  constructor's positional order; keyword construction is unaffected. FastAPI, Flask, NestJS, and
  OpenAPI-imported sources state presence once, so their SDKs are byte-identical either way — as is
  every artifact committed in this repository, whose fixtures and examples contain no field carrying
  both a validation rule and an omission option.

### Breaking

`TsSdk` generation now fails for a map query parameter that carries an enum rule. A TypeScript query
parameter has a defined wire encoding only for scalars and one-dimensional scalar arrays, so `TsSdk`
has always rejected a map-shaped one. The enum used to replace the parameter's whole schema, leaving a
scalar that slipped past that check and emitted a `string` parameter for an API that takes a map; the
corrected map reaches the check and raises `SdkGen`. The client was wrong before rather than right,
but the failure is new and there is nothing to fall back on — a map query parameter has no TypeScript
encoding at all, so the parameter's shape has to change. `GoSdk` (`map[string]string`) and `PySdk`
(`dict[str, Literal[…]]`) are unaffected.

## 0.5.3 — 2026-08-18

### Fixed

- **Go validation rules that apply *inside* a collection no longer bind the field.** The Go
  extractor read a `binding:`/`validate:` tag as a flat token list, so everything after a `dive`
  — or between `keys` and `endkeys` — was attributed to the field itself. A field tagged
  `binding:"omitempty,dive,keys,required,endkeys,required"` was published as a required property
  even though the tag only forbids empty map keys and values, and a per-element `min`/`max`
  behind a `dive` was offered to the container's constraints. The same blind spot made a bound
  query, header, or form parameter required when only its entries were. Schema fields,
  constraints, and request parameters now read tags through one scope-aware tokenizer
  (`goextract/internal/tags`), so they cannot answer the question differently. `keys` and
  `endkeys` are recognized as scope markers rather than parsed as constraints, which also
  removes the spurious `schema.metadata.unresolved` warnings they produced.

  Scope decides where a rule applies; it does not decide whether gnr8 reports one it cannot
  read. A rule gnr8 lowers is dropped in silence when it belongs to an element the graph has
  nowhere to carry, but an unrecognized rule such as `validate:"dive,email"` — or a recognized
  one whose value is missing or malformed, such as `validate:"dive,gte=abc"` — still raises
  `schema.metadata.unresolved`, the same diagnostic each has always raised without the `dive`.

  **This changes emitted documents.** A field whose only `required` sits behind a `dive` leaves
  the schema's `required` array, and a parameter of that shape stops being a required parameter.
  Both are corrections — the tag never said what gnr8 read it to say — but a client generated
  from the new document will accept requests the old one rejected, so review the regenerated
  document before publishing. The affected fields are the ones whose only `required` sits behind
  a `dive` or between `keys` and `endkeys`.

## 0.5.2 — 2026-08-17

### Fixed

- **Go fields tagged `json:",omitzero"` are now source-optional.** The Go extractor previously
  recognized only `omitempty`, so SDK targets could require JSON keys that `encoding/json`
  intentionally omits. `omitzero` now carries the same key-presence meaning while remaining
  independent from nullability.

## 0.5.1 — 2026-08-16

### Fixed

- **`gnr8 check` no longer reports false drift from a warm project cache.** The Go source-analysis
  cache keyed only on the configured input directory, but `go/packages` type-checks the input
  packages together with everything they import — a type defined in a sibling package reaches the
  extracted schemas. An edit to such a package left the key unchanged, so a restored cache answered
  with a stale graph and `check` reported drift against artifacts that were in fact up to date. The
  key now covers the whole enclosing Go module (the nearest `go.mod` at or above the input dir,
  resolved no higher than the project root), plus the input dir itself, the goextract sidecar hash,
  and the Go toolchain identity — `go/packages` type-checks with whatever `go` is on PATH, so the
  selected toolchain decides the stdlib type information and the build constraints that pick which
  files compile. When the module root sits above the project root, a Go workspace (`go.work` /
  `GOWORK`) puts other modules in scope, or the module tree cannot be enumerated exactly, there is no
  cache at all — slower, never wrong. Projects in those shapes will see `check` and `generate` re-run
  Go extraction every time.
- **A restored cache entry can no longer decide a verdict.** Each stored analysis now records the key
  it was computed under. An entry whose recording does not match the current run is discarded and the
  analysis is recomputed, so a cache directory restored from another commit only ever costs time.
- **`gnr8 check -v` now prints the stale and drifted paths** its failure message promises. They
  previously appeared only at `-vv`, which left a failing CI gate with a count and no paths.

### Changed

- The **`gnr8 check` action** now honors a repository-root `rust-toolchain.toml` (or `rust-toolchain`)
  pin by default, so CI compiles the `.gnr8` crate with the same `rustc` as developer machines. The
  new `rust-toolchain: auto` default resolves the pin and uses `stable` when there is none; passing
  any other value overrides the repository pin as before.
- The action's cache `restore-keys` no longer include a prefix-only fallback. Every fallback keeps the
  `.gnr8` crate hash, so a restored entry cannot come from an unrelated pipeline crate.

## 0.5.0 — 2026-08-10

### Added

**Operation prose from handler doc comments.** `summary` and `description` are now read from the
routed handler's own documentation comment — a Go doc comment, a Python docstring, or a TypeScript
JSDoc leading description — as plain prose. The first sentence becomes the `summary`; the remainder
becomes the `description`. The same rule applies in all three languages.

Adding an endpoint no longer requires a `.gnr8/` edit to document it.

- There is **no directive syntax, no marker prefix, and no key/value grammar** inside the comment —
  not `@Summary`, not `gnr8:summary`, not anything. A comment adds words and nothing else; method,
  path, params, body, responses, status codes, tags, security, and operationId remain code-inferred
  and a comment can neither state nor override them.
- Only **routed** handlers are read, so an internal helper's doc comment cannot reach the API surface.
- Go strips a leading `funcName ` and capitalizes the remainder, matching Go's own doc convention.
  TypeScript reads the JSDoc leading description only, so JSDoc **tags are excluded by the compiler**
  and an `@openapi`/`@param` block is invisible to gnr8.
- The prose reaches the OpenAPI document **and** the generated SDKs: Go method comments, Python
  docstrings, and TypeScript JSDoc. An IDE now shows the same words as the spec.
- New opt-in transform **`RequireOperationDocs`** fails generation when any remaining operation has
  no summary, naming its id, method, path, and handler. Off by default. It is a pipeline stage, not
  a source-level check, so place it after your own public-surface filters.

### Fixed

- The OpenAPI YAML writer emitted multi-line strings as plain scalars, putting continuation lines at
  column 0 and producing an invalid document. Multi-line values are now JSON-escaped. Latent before
  this release; multi-line operation descriptions make it reachable.
- `gnr8 generate` now reconstructs its disposable local ownership manifest after cache loss by
  adopting byte-identical emitted files without rewriting them. Divergent files remain protected and
  make generation exit non-zero instead of reporting false success. `generate`, `check`, `doctor`, and
  `watch` now use the same classification rule, and verified no-op state cannot bypass ownership.
- Ownership-manifest updates now atomically replace the prior manifest on Windows as well as Unix, so
  a second generation can publish its reconciled ownership state on every supported platform.
- `--force` now acts only on exact emitted or manifest-owned paths. It no longer recursively deletes
  unrelated files that happen to share an output directory.

### Breaking

`gnr8 generate --accept-generated-baseline` and the `baseline_adopted` JSON field were removed. No
replacement flag is needed: byte-identical output adoption is normal generation behavior, while
`--force` remains the explicit way to overwrite divergent emitted files.

The generated-SDK surface and the code-as-config API are now purely native. Everything whose
purpose was to make gnr8's output resemble another generator's is gone, permanently — see rule 0
in [`CLAUDE.md`](CLAUDE.md), enforced by `make invariants`.

`DocumentOperation::summary()` / `::description()` now **error** when they target an operation that
already has prose from its source (its handler's doc comment, or an imported spec). An operation's
prose has exactly one source; two ways to state one fact is the defect, and picking a winner between
them is the same defect with extra steps (rule 3). They remain the supported path for operations that
have no source prose. `OperationDocsPolicy` no longer carries `summary`/`description` — prose lives on
`Operation`, so duplication is structurally impossible rather than merely discouraged.

Removed public API (no replacement; these were compatibility surface):

- Modules `gnr8::sdk::go`, `gnr8::sdk::openapi_compat`, `gnr8::sdk::profile`, `gnr8::sdk::surface`,
  `gnr8::sdk::typescript`.
- `SdkProfile`, `SdkTypeAliases`, `SdkOperationAliases`, `OpenApiSchemaAliases`, `QueryParam`.
- Go target controls `RequiredPointerConstructorPolicy`, `QueryTimeFormat`, `GoRequestBuilderScope`,
  `GoRequestBuilderAliases`, `GoRequestBuilderOperationAliases`, `GoQuerySetterArgumentPolicy`,
  `GoExecuteCompatibility`.
- TypeScript target controls `TsModelPropertyPolicy`, `TsNullablePolicy`, `TsResponsePolicy`,
  `TsBarrelExports`.
- Builder methods `.aliases()`, `.profile()`, `.type_alias()`, `.error_model()`,
  `.schema_aliases()`, `.request_body_param_name()`, `.init_override_function()` on the SDK and
  OpenAPI targets, and `ApiOverrides::query_param()`.
- `DiagnosticCategory::Compatibility`, `runner::BUNDLE_VERSION`, the deprecated `Artifacts::write`,
  and `Pipeline::post_write`.

Migration: use `RenameOperation` / `RenameType` to choose one canonical public name,
`SdkFileLayout` / `OperationFileSplit` / `SdkPackageMetadata` for shape, `RequestParameter` +
`ParameterOverride` in place of `QueryParam`, and `Artifacts::create` / `overlay` / `rewrite` in
place of `Artifacts::write`. Preserving a previous generator's exact SDK surface is a non-goal.

Behaviour changes in generated SDKs:

- **TS: optional query positionals → single params object.** A TypeScript operation no longer
  expands its query and header parameters into a positional argument list. Path parameters stay
  positional and come first, the typed request body stays its own argument, and every other request
  parameter arrives as one exported `{OperationId}Params` object, with `RequestOptions` last. The
  params object is optional (`params?: FooParams`) when every parameter is optional and required
  (`params: FooParams`) when any one of them is. An operation with no such parameters is unchanged
  and gains no empty argument. Query wire names, styles, `explode`, and the emitted query string are
  identical; Go and Python are untouched.

  Updating call sites: pass the arguments you were passing by name instead of by position, and drop
  the `undefined` placeholders.

  ```ts
  // before
  await client.getItemsPaginated(undefined, cursor, undefined, undefined, 50);
  // after
  await client.getItemsPaginated({ cursor, pageSize: 50 });

  // before
  await client.getItem(itemId, true);
  // after
  await client.getItem(itemId, { verbose: true });
  ```

  Each params type is re-exported from the package root (`import type { GetItemsPaginatedParams }
  from "@acme/sdk"`), so a caller can name the object it builds.
- Operations whose security cannot be satisfied now fail before the request is built
  (`AuthConfigurationError` in all three languages) instead of silently sending an unauthenticated
  request. Clients that supply credentials from the transport (a signing round-tripper, an
  authenticating proxy, a request hook) opt out with `WithTransportAuth()` (Go),
  `auth_transport=True` (Python), or `authMode: "transport"` (TypeScript).
- Go no longer emits the duplicate suffix-style enum constants (`FictionGenre`); only the prefix
  form (`GenreFiction`) remains.
- **Go: a pluralized initialism keeps its capitals.** `stepUuids` emitted `StepUuids` while the
  singular `uuid` correctly emitted `UUID`. Every exported Go identifier gnr8 derives — model
  fields, `*Params` struct fields, method names — now spells the `ID`/`UUID`/`URL`/`API` families
  full caps when pluralized: `StepUUIDs`, `LabelIDs`, `SiteURLs`, `PublicAPIs`. This changes the Go
  identifier ONLY. Json tags, query wire names, path templates, and OpenAPI property names are
  untouched, and TypeScript (`stepUuids`) and Python (`step_uuids`) keep their own language-native
  spelling. Use `RenameType` / `RenameOperation` if you want a different canonical name.
- TypeScript enums are emitted as a runtime const object plus a derived type, so the name is now a
  value export rather than a type-only export.
- `204` responses are treated as bodyless.

Wire format: `Artifact::producer` is required in the artifact bundle; bundles written by older
versions no longer deserialize.

### Fixed

- A plural initialism in the MIDDLE of an identifier tokenized one letter short, so `userUUIDsList`
  split into `UUI` + `Ds` and produced `uui_ds` file stems, `UserUUIDsList`-by-accident Go names,
  and `userUuiDsList` TypeScript identifiers. The pluralizing `s` now stays with its acronym
  wherever the acronym sits, in all three languages.
- A generated TypeScript cursor-pagination generator bound the page's item list without reading it,
  so the SDK did not compile under `--noUnusedLocals`. The list is now bound only where the loop
  uses it (empty-page termination and the offset advance).
- The split-layout TypeScript SDK was not Prettier-clean: every operation body was written at the
  class-method depth even though a split operation is a module-level function, leaving each line
  two columns too far right. The Prettier gate now covers the split layout as well as the compact
  one, the way the Python gate already covered both.
- A TypeScript path parameter named `path`, `headers`, or `res` silently emitted a method that could
  not compile, because each shadows or redeclares a local the body binds. Those names now raise the
  same typed error `body` already did.
- **TypeScript SDKs were unusable in browsers.** The global `fetch` was captured unbound and then
  invoked as a method, so the receiver was the client and every call threw `Illegal invocation`.
- **Retry waits were unbounded and uninterruptible** in all three languages. Total time spent
  waiting between retries is now capped at 60s across the whole retry sequence — a longer
  `Retry-After` is honoured only up to that cap — and the wait observes cancellation in Go and
  TypeScript. Transport-error retries back off exponentially instead of reconnecting instantly.
  Note that `timeout` bounds the whole call in Go but each individual attempt in Python and
  TypeScript; each generated README now states which.
- TypeScript did not release discarded retry response bodies, holding sockets out of the pool.
- Python matched response headers case-sensitively, so a server sending `X-Request-Id` or
  `Retry-after` — both common — was silently missed. Header lookups are case-insensitive now, as
  HTTP requires. Go and TypeScript were already correct.
- The `ApiError` handed to error hooks carried no request id in Python and TypeScript, while the
  error thrown to the caller did, so one failure looked different depending on how it was observed.
- A `204` response that declared a body was silently dropped by the SDK emitters while the OpenAPI
  lowering kept it, so one graph produced two artifacts describing different contracts. The
  contradiction is now rejected with a typed error.
- The anonymous security alternative could not survive lowering: an empty group produced no rows,
  so the emitted document claimed authentication was mandatory while the SDKs treated it as
  optional.
- Optional security (`security: [{}, {Scheme: []}]`) silently sent no credentials, because the
  always-satisfiable anonymous alternative shadowed every credentialed one.
- Security alternatives were reordered alphabetically, discarding the author's preference order.
- Generated Python referenced credential attributes that were never assigned, raised
  `OverflowError` for very large `max_retries`, omitted `AuthConfigurationError` from the package
  barrel, emitted path parameters without type annotations, and tripped `RET503`.
- Generated TypeScript failed `tsc --noUnusedLocals`, `--exactOptionalPropertyTypes`, and
  `--noUncheckedIndexedAccess`; all three are now gated in CI.
- Go: the request context was cancelled before the caller could read the response body; non-string
  path parameters did not compile; multipart parts now carry real filenames.
- Go middleware is now resolved in statement order, so a `Use(...)` registered after a route no
  longer secures it retroactively, and middleware propagates through helper functions that take a
  `*gin.RouterGroup`.
- The Go extractor no longer reads the `swaggertype` struct tag; a source type's real Go type is
  the only source of truth.
- Failed subprocesses report the actionable end of their output instead of only the first line.
