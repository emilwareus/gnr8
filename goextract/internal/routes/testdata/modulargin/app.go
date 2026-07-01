package modulargin

import "github.com/gin-gonic/gin"

type Server struct {
	Router *gin.Engine
	Guard  gin.HandlerFunc
}

func (s Server) Register() {
	api := s.Router.Group("/api")
	api.Use(s.Guard)
	api.GET("/health", s.health)

	books := api.Group("/books")
	books.GET("/", s.listBooks)
	books.GET("/:id", s.getBook)
}

func (s Server) health(c *gin.Context)    { _ = c }
func (s Server) listBooks(c *gin.Context) { _ = c }
func (s Server) getBook(c *gin.Context)   { _ = c }
