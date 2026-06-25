// Package ports contains the HTTP adapter for the goal domain: Gin route
// registration and handlers. It is the primary input gnr8's analyzer reads to
// derive the route table (method + path template + params + request/response
// types + status codes) and the group-level security requirement.
//
// Recognized Gin patterns (CONTEXT D-02):
//   - h.Router.Group("/" + basePath)        -> base path / server prefix
//   - api.Use(h.AuthMiddleware)             -> security on the whole group (D-14)
//   - api.METHOD(path, handler)             -> one route per registration
//   - ":uuid" path segment                  -> path parameter named "uuid"
package ports

import "github.com/gin-gonic/gin"

// HttpServer is the goal-domain HTTP adapter. Router holds the Gin engine the
// routes are registered on, and AuthMiddleware is the group-level guard whose
// presence implies a security requirement on every operation in the group.
type HttpServer struct {
	Router         *gin.Engine
	AuthMiddleware gin.HandlerFunc
}

// NewHttpServer constructs an HttpServer with a default (no-op) auth middleware
// so the module compiles and the route group always has a middleware to attach.
func NewHttpServer(router *gin.Engine) HttpServer {
	return HttpServer{
		Router:         router,
		AuthMiddleware: defaultAuthMiddleware(),
	}
}

// defaultAuthMiddleware is a trivial mock guard. It never runs in production —
// the fixture is analyzer INPUT only (threat T-02-01) — it exists so the group
// has an auth middleware for the extractor to observe (D-14).
func defaultAuthMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		// A real guard would validate an API key / token here and abort on
		// failure. The fixture simply continues.
		c.Next()
	}
}

// setupRoutes registers the goal domain's four routes under "/<basePath>" with
// the auth middleware applied to the whole group. This mirrors TARGET-API.md
// §3.1 exactly:
//
//	POST   /goal/        -> createGoal
//	GET    /goal/list    -> listGoals
//	PUT    /goal/{uuid}  -> updateGoal
//	DELETE /goal/{uuid}  -> deleteGoal
func (h HttpServer) setupRoutes(basePath string) {
	api := h.Router.Group("/" + basePath)
	api.Use(h.AuthMiddleware) // security applies to the whole group (D-14)
	{
		api.POST("/", h.createGoal)        // POST   /goal/
		api.GET("/list", h.listGoals)      // GET    /goal/list
		api.PUT("/:uuid", h.updateGoal)    // PUT    /goal/{uuid}
		api.DELETE("/:uuid", h.deleteGoal) // DELETE /goal/{uuid}
	}
}

// RegisterGoalRoutes is an exported entrypoint that wires the goal routes onto a
// server. Keeping setupRoutes referenced here ensures the module has no unused
// methods and gives the analyzer a public registration seam to start from.
func (h HttpServer) RegisterGoalRoutes(basePath string) {
	h.setupRoutes(basePath)
}
