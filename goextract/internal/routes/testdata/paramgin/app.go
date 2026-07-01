package paramgin

import "github.com/gin-gonic/gin"

type Server struct {
	Router *gin.Engine
}

func (s Server) Register() {
	api := s.Router.Group("/api")
	registerBooks(api.Group("/books"))
}

func registerBooks(group *gin.RouterGroup) {
	group.GET("/:id", getBook)
}

func getBook(*gin.Context) {}
