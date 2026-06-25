package main

import "time"

// Genre is a code-defined string enum. gnr8 reads the `const` set below straight
// from go/types and emits it as an OpenAPI string enum — no annotation needed.
type Genre string

const (
	GenreFiction    Genre = "fiction"
	GenreNonfiction Genre = "nonfiction"
	GenreSciFi      Genre = "scifi"
	GenreMystery    Genre = "mystery"
	GenreRomance    Genre = "romance"
)

// Publisher is a nested DTO referenced by Book. Because it carries json tags it
// becomes its own component schema and Book.publisher resolves to a $ref.
type Publisher struct {
	Name    string `json:"name"`
	Country string `json:"country"`
}

// Book is the core resource. Every field is a plain typed value, so each one maps
// cleanly to an OpenAPI schema: the Genre enum, a float64 price, a time.Time, an
// optional *string subtitle (omitempty), a nested Publisher, and a []string of tags.
type Book struct {
	ID          string    `json:"id"`
	Title       string    `json:"title"`
	Author      string    `json:"author"`
	Genre       Genre     `json:"genre"`
	Price       float64   `json:"price"`
	PublishedAt time.Time `json:"publishedAt"`
	Subtitle    *string   `json:"subtitle,omitempty"`
	Publisher   Publisher `json:"publisher"`
	Tags        []string  `json:"tags"`
}

// CreateBookRequest is the POST /books body. `binding:"required"` marks the
// required fields gnr8 surfaces in the OpenAPI `required` list.
type CreateBookRequest struct {
	Title     string    `json:"title" binding:"required"`
	Author    string    `json:"author" binding:"required"`
	Genre     Genre     `json:"genre" binding:"required"`
	Price     float64   `json:"price"`
	Subtitle  *string   `json:"subtitle,omitempty"`
	Publisher Publisher `json:"publisher"`
	Tags      []string  `json:"tags"`
}

// UpdateBookRequest is the PUT /books/:id body. All fields are optional updates.
type UpdateBookRequest struct {
	Title    *string  `json:"title,omitempty"`
	Author   *string  `json:"author,omitempty"`
	Genre    *Genre   `json:"genre,omitempty"`
	Price    *float64 `json:"price,omitempty"`
	Subtitle *string  `json:"subtitle,omitempty"`
	Tags     []string `json:"tags,omitempty"`
}

// BookList is the GET /books response envelope.
type BookList struct {
	Books []Book `json:"books"`
}

// ErrorResponse is the error envelope returned for 400 / 404 responses.
type ErrorResponse struct {
	Message string `json:"message"`
	Code    string `json:"code"`
}
