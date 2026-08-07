// Command bookstore is a tiny Gin service that gnr8 reads to generate an OpenAPI
// 3.1 document and a Go SDK. gnr8 derives every fact from the Go code itself
// (routes, request/response types, status codes, the Genre enum) and takes the
// base path + security scheme from the .gnr8/ Rust lifecycle (code, not config).
package main

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

func main() {
	r := gin.Default()
	registerRoutes(r)
	_ = r.Run(":8080")
}

// registerRoutes mounts a static route group at /books. gnr8 extracts that group
// prefix from source; SetBasePath is reserved for an external mount prefix.
func registerRoutes(r *gin.Engine) {
	books := r.Group("/books")
	{
		books.POST("", createBook)
		books.GET("", listBooks)
		books.GET("/:id", getBook)
		books.PUT("/:id", updateBook)
		books.DELETE("/:id", deleteBook)
	}
}

// createBook adds a book to the catalogue.
//
// The book is stored immediately and returned with its generated identifier.
func createBook(c *gin.Context) {
	var req CreateBookRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, ErrorResponse{Message: "invalid request body", Code: "bad_request"})
		return
	}
	book := Book{Title: req.Title, Author: req.Author, Genre: req.Genre, Price: req.Price}
	c.JSON(http.StatusCreated, book)
}

// listBooks returns every book in the catalogue.
//
// Pass a genre to narrow the results to one genre; omit it to list everything.
func listBooks(c *gin.Context) {
	genre := c.Query("genre")
	_ = genre
	c.JSON(http.StatusOK, BookList{Books: []Book{}})
}

// getBook returns one book by its identifier.
func getBook(c *gin.Context) {
	id := c.Param("id")
	if id == "" {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "book not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Book{ID: id})
}

// updateBook replaces the mutable fields of one book.
//
// Fields omitted from the payload keep their current values.
func updateBook(c *gin.Context) {
	id := c.Param("id")
	var req UpdateBookRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "book not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Book{ID: id})
}

// deleteBook permanently removes one book from the catalogue.
func deleteBook(c *gin.Context) {
	id := c.Param("id")
	c.JSON(http.StatusOK, ErrorResponse{Message: "deleted " + id, Code: "ok"})
}
