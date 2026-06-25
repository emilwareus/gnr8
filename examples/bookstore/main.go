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

// registerRoutes mounts the ONE supported route group at /books. gnr8 records the
// routes group-relative; the /books prefix is set in .gnr8/src/main.rs (SetBasePath).
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

// createBook handles POST /books: bind a CreateBookRequest, return the new Book.
func createBook(c *gin.Context) {
	var req CreateBookRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, ErrorResponse{Message: "invalid request body", Code: "bad_request"})
		return
	}
	book := Book{Title: req.Title, Author: req.Author, Genre: req.Genre, Price: req.Price}
	c.JSON(http.StatusCreated, book)
}

// listBooks handles GET /books with an optional ?genre= filter.
func listBooks(c *gin.Context) {
	genre := c.Query("genre")
	_ = genre
	c.JSON(http.StatusOK, BookList{Books: []Book{}})
}

// getBook handles GET /books/:id.
func getBook(c *gin.Context) {
	id := c.Param("id")
	if id == "" {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "book not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Book{ID: id})
}

// updateBook handles PUT /books/:id: bind an UpdateBookRequest, return the Book.
func updateBook(c *gin.Context) {
	id := c.Param("id")
	var req UpdateBookRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "book not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Book{ID: id})
}

// deleteBook handles DELETE /books/:id.
func deleteBook(c *gin.Context) {
	id := c.Param("id")
	c.JSON(http.StatusOK, ErrorResponse{Message: "deleted " + id, Code: "ok"})
}
