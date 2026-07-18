<!-- generated-by: gsd-doc-writer -->
# SDK compatibility

SDK compatibility commands statically compare a baseline directory (`--old`) with a candidate
directory (`--new`). They do not compile, import, or execute the packages. Exit `1` means an
unapproved backward-incompatible public-surface difference was found.

## Commands

```bash
gnr8 compat go --old legacy/go --new generated/go
gnr8 compat typescript --old legacy/ts --new generated/typescript
gnr8 compat python --old legacy/python --new generated/python
```

Go and TypeScript accept a TOML contract and suggestions:

```bash
gnr8 compat go \
  --old legacy/go \
  --new generated/go \
  --contract sdk-compat.toml \
  --suggest
```

Without a contract, every reported backward-incompatible diff fails. With a contract, missing
required symbols and unapproved diff items fail. Stale allowances are reported in JSON but do not
fail; remove them to keep policy minimal. `--suggest` proposes high-confidence contract/config
snippets but never edits files.

## Compared Go surface

- Exported struct, interface, alias, and defined-type declarations.
- Exported constants and variables.
- Exported function signatures.
- Exported methods keyed by `Receiver.Method`.
- Generated documentation file paths.
- `go.mod` and other package metadata.

Declaration and signature comparisons are canonicalized to ignore irrelevant formatting while
retaining public shape.

## Compared TypeScript surface

- Root and model exports, including type/value namespace kind.
- API classes, API factories, operation methods, and request aliases.
- Interface properties: presence, requiredness, nullability, and type.
- Public type aliases, heritage, enums, and enum-like declarations.
- Operation/factory return types and full method signatures.
- Package entry points and generated documentation file paths.

## Compared Python surface

- Importable module paths and names re-exported by package `__init__.py` files.
- Public model classes and annotated fields.
- Field requiredness, nullability, canonical type, and wire alias.
- Explicit model constructors and `from_dict`/`to_dict` helpers.
- Exception classes, public type/model aliases, and package entry points.

Python currently has no contract or `--suggest`; every reported diff fails.

## Contract schema

Unknown fields are rejected. All listed keys are optional arrays except
`allow.docs_layout_migration`, a boolean defaulting to `false`.

```toml
[allow]
docs_layout_migration = false
missing_docs = []

[go]
require_exported_types = []
require_exported_values = []
require_exported_functions = []
require_exported_methods = []
allow_missing_exported_types = []
allow_missing_exported_values = []
allow_missing_exported_functions = []
allow_missing_exported_methods = []
allow_exported_type_changes = []
allow_exported_value_changes = []
allow_exported_function_signature_changes = []
allow_exported_method_signature_changes = []
allow_missing_docs = []
allow_package_metadata_changes = []

[typescript]
require_root_exports = []
require_model_exports = []
require_api_classes = []
require_api_factories = []
require_operation_methods = []
require_request_aliases = []
allow_missing_root_exports = []
allow_missing_model_exports = []
allow_missing_api_classes = []
allow_missing_api_factories = []
allow_missing_operation_methods = []
allow_missing_request_aliases = []
allow_missing_interface_properties = []
allow_interface_property_changes = []
allow_type_declaration_changes = []
allow_operation_return_type_changes = []
allow_operation_signature_changes = []
allow_export_kind_mismatches = []
allow_package_entry_point_changes = []
allow_missing_docs = []
```

Item identities follow JSON diff keys. Examples:

- Go methods: `Client.ListBooks`; changed types/functions use the exported symbol name.
- TypeScript interface fields: `Book.title`.
- TypeScript methods: `BooksApi.listBooks`.
- Docs and package-entry changes: the exact path/key printed by the comparator.

Global `allow.missing_docs` applies to Go and TypeScript. Set `docs_layout_migration = true` only for
a reviewed one-time layout replacement; it approves all missing docs.

## JSON workflow

```bash
gnr8 --json compat typescript \
  --old legacy/ts \
  --new generated/typescript \
  --contract sdk-compat.toml \
  --suggest > compat.json
status=$?
```

The result includes baseline/candidate paths, raw surface diff, compatibility/breaking decision,
contract evaluation when supplied, stale allowances, and suggestions when requested. Always use the
process status as the gate.

## Migration strategy

1. Generate with the nearest compatibility profile.
2. Run the comparator without a contract to obtain the complete diff.
3. Prefer target compatibility controls and explicit aliases for real legacy API requirements.
4. Add a contract allowance only for intentional, reviewed drift that cannot or should not be
   reproduced.
5. Add `require_*` entries for critical symbols so a too-small baseline cannot make the gate pass.
6. Remove stale allowances as the diff disappears.
7. Run the language compiler/tests separately; static surface compatibility does not prove runtime
   behavior.

Useful target controls are documented in [SDK generation](generation.md). OpenAPI uses a separate
zero-difference gate described in [OpenAPI compatibility](../openapi/compatibility.md).
