// Typed DTO classes for the NestJS bookstore fixture.
//
// BRIGHT LINE (CLAUDE.md rule 1, the whole product premise): every API fact gnr8
// will later extract from these DTOs is carried by ORDINARY TypeScript property
// types — string-literal-union enums, `A | B` unions, `field?: T` optional vs
// `field: T | null` nullable. There is deliberately NO third-party
// schema-annotation decorator and NO separate validation-schema dialect on these
// classes. gnr8 derives the same facts the language's own type system already
// carries, via the language's reference compiler (Phase 4) — never a sidecar
// convention tool.
//
// OPTIONAL vs NULLABLE — the two distinct axes (Plan 01-01):
//   * optional = the property may be absent  (TS `field?: T`).
//   * nullable = the value may be `null`     (TS `field: T | null`).
// Both, neither, and each alone all appear on `BookFilters` below.

// ----- cross-language enums (string-literal unions) -----------------------

// A string-literal-union enum -> neutral `Type::Enum` (members sorted:
// [hardcover, paperback]). Declared out of lexical order on purpose.
export type BookFormat = 'paperback' | 'hardcover';

// Another string-literal union, used on the filters DTO.
export type SortOrder = 'asc' | 'desc';

// ----- objects (DTO classes) ----------------------------------------------

// A nested DTO referenced by `BookDto` -> a `$ref` to this schema.
export class AuthorDto {
  // neither: required, non-null
  name: string;
  // nullable only: value may be null, property always present
  bio: string | null;
}

// The densest DTO: objects, arrays, an enum field, and a union field.
export class BookDto {
  id: number; // neither
  title: string; // neither
  author: AuthorDto; // nested object $ref (neither)
  // optional only: property may be absent, value never null
  tags?: string[];
  format: BookFormat; // string-literal-union enum (neither)
  // both: optional (`?`) AND the type admits null -> a union of two primitives
  rating?: number | null;
}

export class BookFilters {
  // neither: required, non-null
  genre: string;
  // optional only: `?`, value never null
  inStock?: boolean;
  // nullable only: value may be null, property always present
  published: number | null;
  // both: optional `?` AND nullable `| null`; also a string-literal-union enum
  sort?: SortOrder | null;
}

// One arm of a union of two OBJECTS (NestJS/TS has real sum types, unlike Go).
export class OutOfStockDto {
  reason: string;
}

// `BookDto | OutOfStockDto` -> neutral `Type::Union` of two named ($ref) members.
export type BookOrError = BookDto | OutOfStockDto;

export class CreatedMessage {
  message: string; // neither
  id: number; // neither
}

export class ListBooksResponse {
  books: BookDto[]; // array of object $refs (neither)
  nextCursor: string | null; // nullable only
  total: number; // neither
}
