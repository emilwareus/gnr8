// Package aliasedgin registers Gin routes while importing gin under an ALIAS
// (`grouter`). It exists to prove the route recognizer resolves the Gin method
// identity via go/types (receiver package path), not by string-matching the
// import name — recognition must survive `import grouter "...gin"`.
package aliasedgin

import grouter "github.com/gin-gonic/gin"

// Server holds the engine + a middleware so the group has something to Use.
type Server struct {
	Router *grouter.Engine
	Guard  grouter.HandlerFunc
}

// register wires two routes (one with a path param) under a secured group, using
// the aliased gin import throughout.
func (s Server) register(basePath string) {
	api := s.Router.Group("/" + basePath)
	api.Use(s.Guard)
	{
		api.POST("/", s.create)
		api.GET("/:id", s.read)
	}
}

func (s Server) create(c *grouter.Context) { _ = c }
func (s Server) read(c *grouter.Context)   { _ = c }
