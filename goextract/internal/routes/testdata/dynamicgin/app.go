package dynamicgin

import "github.com/gin-gonic/gin"

func Register(router *gin.Engine, prefix string) {
	api := router.Group(prefix)
	api.GET("/ping", ping)
}

func ping(c *gin.Context) {}
