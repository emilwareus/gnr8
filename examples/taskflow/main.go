// Command taskflow is a small Gin "tasks" service that gnr8 reads to generate an
// OpenAPI 3.1 document, a Go SDK, and (via a user-written generator in .gnr8/) an
// API.md summary. gnr8 derives every API fact from the Go code itself — routes,
// request/response types, status codes, the Status enum — and takes the base path
// + security scheme from the .gnr8/ Rust lifecycle (code, not config).
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

// registerRoutes mounts a static route group at /tasks. gnr8 extracts that group
// prefix from source; SetBasePath is reserved for an external mount prefix.
// Note the /_debug route: it is a real internal endpoint that the
// .gnr8/ pipeline keeps in every target and marks with the standard "internal" tag.
func registerRoutes(r *gin.Engine) {
	tasks := r.Group("/tasks")
	{
		tasks.POST("", createTask)
		tasks.GET("", listTasks)
		tasks.GET("/:id", getTask)
		tasks.PUT("/:id", updateTask)
		tasks.DELETE("/:id", deleteTask)

		// An internal diagnostics endpoint. It stays in the API surface and carries
		// a standard tag so change-gate policy remains explicit at invocation time.
		tasks.GET("/_debug", debugTasks)
	}
}

// createTask creates a task.
//
// The task starts in the pending status and is returned with its generated
// identifier.
func createTask(c *gin.Context) {
	var req CreateTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, ErrorResponse{Message: "invalid request body", Code: "bad_request"})
		return
	}
	task := Task{Title: req.Title, Status: req.Status, Priority: req.Priority, DueAt: req.DueAt, Assignee: req.Assignee, Labels: req.Labels}
	c.JSON(http.StatusCreated, task)
}

// listTasks returns every task.
//
// Pass a status to narrow the results to one status; omit it to list everything.
func listTasks(c *gin.Context) {
	status := c.Query("status")
	_ = status
	c.JSON(http.StatusOK, TaskList{Tasks: []Task{}})
}

// getTask returns one task by its identifier.
func getTask(c *gin.Context) {
	id := c.Param("id")
	if id == "" {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "task not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Task{ID: id})
}

// updateTask replaces the mutable fields of one task.
//
// Fields omitted from the payload keep their current values.
func updateTask(c *gin.Context) {
	id := c.Param("id")
	var req UpdateTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "task not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Task{ID: id})
}

// deleteTask permanently removes one task.
func deleteTask(c *gin.Context) {
	id := c.Param("id")
	c.JSON(http.StatusOK, ErrorResponse{Message: "deleted " + id, Code: "ok"})
}

// debugTasks handles the internal GET /tasks/_debug endpoint. The .gnr8/ pipeline
// keeps this route generated so change reporting can apply explicit tag-based gate policy.
func debugTasks(c *gin.Context) {
	c.JSON(http.StatusOK, TaskList{Tasks: []Task{}})
}
