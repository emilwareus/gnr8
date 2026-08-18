// Package tags reads the `binding:`/`validate:` struct tags Go web stacks attach to
// request DTOs, and answers one question about them: which value does a given rule
// constrain?
//
// A tag is a comma-separated list, but it is not flat. `dive` steps into the field's
// elements and `keys`…`endkeys` selects a map's keys, so a rule's position decides
// what it talks about: tokens before the first `dive` constrain the field, tokens
// after it constrain something inside the field. Reading a tag without that
// structure attributes an element's rule to its container — an element-level
// `required` makes an optional key required, a per-element bound is published as the
// array's bound.
//
// Honouring the three scope markers therefore narrows what gnr8 reads rather than
// widening it. The schema required axis, the constraint parser, and the request
// parameter required axis all go through Scoped, so they cannot answer the same
// question differently.
//
// One reader does not: handlers.parameterEnumValues takes the first `oneof=` rule at
// any scope. For an array parameter that lands correctly either way, because the
// enum is placed on the element; for a map parameter a post-`dive` `oneof` replaces
// the whole schema. Routing it through Scoped means deciding where an element-scope
// enum belongs in a map — the value schema, or the key schema after `keys` — which
// is a design question, not a bug fix. Until that is settled it is the one scope
// blind spot left, and it is tracked rather than quietly tolerated.
package tags

import "strings"

// Scope names which value a tag token constrains.
type Scope int

const (
	// ScopeField is the field itself — the only scope the neutral graph records.
	ScopeField Scope = iota
	// ScopeElement is a value inside the field's collection.
	ScopeElement
	// ScopeMapKey is a key of the field's map.
	ScopeMapKey
)

// Token is one comma-separated token of a tag value paired with the scope it
// applies to. Scope markers are structural and never appear as a Token.
type Token struct {
	Text  string
	Scope Scope
}

// Scoped splits a `binding:`/`validate:` value into tokens and marks what each one
// talks about.
func Scoped(value string) []Token {
	if value == "" {
		return nil
	}
	parts := strings.Split(value, ",")
	tokens := make([]Token, 0, len(parts))
	dived := false
	inKeys := false
	for _, part := range parts {
		text := strings.TrimSpace(part)
		switch text {
		case "":
			continue
		case "dive":
			// One dive already leaves the field; nested dives only descend further.
			dived = true
			inKeys = false
			continue
		case "keys":
			inKeys = true
			continue
		case "endkeys":
			inKeys = false
			continue
		}
		scope := ScopeField
		switch {
		case inKeys:
			scope = ScopeMapKey
		case dived:
			scope = ScopeElement
		}
		tokens = append(tokens, Token{Text: text, Scope: scope})
	}
	return tokens
}

// HasFieldToken reports whether the tag states the given rule about the field
// itself. A matching token reached through `dive` or `keys` describes what lives
// inside the field and does not count.
func HasFieldToken(value string, expected string) bool {
	for _, token := range Scoped(value) {
		if token.Scope == ScopeField && token.Text == expected {
			return true
		}
	}
	return false
}
