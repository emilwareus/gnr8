# Changelog

Notable changes to `gnr8`. Entries under **Unreleased** land in the next release; the
[release runbook](docs/RELEASE.md) chooses `patch` or `minor` based on whether this section
contains a **Breaking** heading.

Cargo treats `0.x.y → 0.x.(y+1)` as a compatible upgrade, so any release with breaking changes
must move the minor version.

## Unreleased

### Breaking

The generated-SDK surface and the code-as-config API are now purely native. Everything whose
purpose was to make gnr8's output resemble another generator's is gone, permanently — see rule 0
in [`CLAUDE.md`](CLAUDE.md), enforced by `make invariants`.

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
