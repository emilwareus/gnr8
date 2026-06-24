# SDK Generation And Structure

## Research Question

What SDK structure should `gnr8` generate, especially for the first Go SDK backend?

## Position

SDK generation should be owned by `gnr8` and driven from the internal API graph. OpenAPI can be emitted for interoperability, but the SDK generator should not depend on re-parsing OpenAPI that `gnr8` just produced.

The first SDK backend should be excellent before the project adds more targets. Research TypeScript, Python, and Rust now, but do not implement them until the Go SDK shape is validated.

## Good SDK Properties

A good generated SDK should be:

- Idiomatic for the target language.
- Type-safe.
- Small enough to inspect.
- Predictable in file layout and naming.
- Easy to debug.
- Easy to customize without forking the generator.
- Clear about auth, retries, errors, pagination, and transport.
- Stable across regeneration.

Speakeasy's SDK best-practices writing emphasizes type safety, abstraction, readability, limited dependencies, and enterprise features as qualities of strong SDKs.

Source: <https://www.speakeasy.com/blog/sdk-best-practices>

Speakeasy's Go SDK methodology also emphasizes minimal dependencies, type safety, and debugging.

Source: <https://www.speakeasy.com/docs/sdks/languages/golang/methodology-go>

Stripe's Go SDK recommends a client-centered access pattern through `stripe.Client`, with options for custom backends.

Source: <https://github.com/stripe/stripe-go/blob/master/README.md>

## Proposed Go SDK Shape

Candidate structure:

```text
sdk/
  client.go
  option.go
  transport.go
  error.go
  pagination.go
  models/
    user.go
    invoice.go
  operations/
    users.go
    invoices.go
  internal/
    encode.go
    decode.go
```

Open questions:

- Should models be a subpackage or same package?
- Should operations be resource services on a client, e.g. `client.Users.Create(...)`, or flat client methods, e.g. `client.CreateUser(...)`?
- Should request/response models be separate from domain schemas?
- Should generated code optimize for human readability or minimal package count?

## Client API Options

### Flat Client

Example:

```go
client.CreateUser(ctx, request)
client.GetUser(ctx, id)
```

Pros:

- Simple.
- Easy for small APIs.
- Fewer types.

Cons:

- Does not scale well with large APIs.
- Names can become long or collide.

### Resource Services

Example:

```go
client.Users.Create(ctx, request)
client.Users.Get(ctx, id)
```

Pros:

- Scales better.
- Maps to tags/resources.
- Easier to browse.

Cons:

- More generated structure.
- Requires stable resource grouping.

### Builder Pattern

Example:

```go
client.Users.Get(id).WithExpand("roles").Do(ctx)
```

Pros:

- Good for optional parameters and complex calls.
- Can improve discoverability.

Cons:

- More generated code.
- Less direct for simple APIs.

## SDK Feature Areas

### Transport

Must support:

- Custom `http.Client`.
- Base URL override.
- Request hooks.
- Response hooks.
- Test transport injection.

### Authentication

Must support:

- Static bearer token.
- API key headers.
- Per-request auth overrides.
- Custom signing hooks.

### Errors

Must generate:

- Typed API error.
- Status code.
- Response body.
- Request ID headers when present.
- Decode failure information.

### Retries

Must decide:

- Default retry policy or opt-in only.
- Idempotency handling.
- Backoff customization.
- Which status codes are retryable.

### Pagination

Must handle:

- Cursor pagination.
- Page/page-size pagination.
- Link-header pagination.
- Generated iterators where idiomatic.

### Streaming

Must consider OpenAPI 3.2 streaming support and language-specific streaming APIs.

## Customization Problem

SDK customization should be code. Users should be able to customize:

- Naming.
- Grouping.
- Auth.
- Retry policy.
- Transport.
- Error model.
- Pagination model.
- File layout.
- Package/module metadata.
- Hand-written extension files.

This points toward `.gnr8/` SDK backend code rather than config files.

## Research Tasks

1. Compare generated Go SDK layouts from Stripe, Speakeasy, OpenAPI Generator, and oapi-codegen.
2. Decide whether Go backend should use flat methods or resource services by default.
3. Define customization hooks.
4. Define generated-file ownership boundaries.
5. Define how user-written SDK extensions survive regeneration.
6. Define what target-language assumptions belong in SDK backends, not in the core graph.
