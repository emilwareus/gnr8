package modulargin

import "github.com/gin-gonic/gin"

func (s Server) RegisterAdmin() {
	admin := s.Router.Group("/api").Group("/admin")
	admin.GET("/stats", s.adminStats)
}

func (s Server) RegisterDirect(router *gin.Engine) {
	router.Group("/api").GET("/ready", s.ready)
}

func (s Server) adminStats(c *gin.Context) { _ = c }
func (s Server) ready(c *gin.Context)      { _ = c }
