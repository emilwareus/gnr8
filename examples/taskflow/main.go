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

// registerRoutes mounts the ONE supported route group at /tasks. gnr8 records the
// routes group-relative; the /tasks prefix comes from the .gnr8/ lifecycle
// (SetBasePath). Note the /_debug route: it is a real internal endpoint that the
// .gnr8/ lifecycle's custom DropDebugRoutes transform removes before generation,
// so it never reaches the OpenAPI document or the SDK.
func registerRoutes(r *gin.Engine) {
	tasks := r.Group("/tasks")
	{
		tasks.POST("", createTask)
		tasks.GET("", listTasks)
		tasks.GET("/:id", getTask)
		tasks.PUT("/:id", updateTask)
		tasks.DELETE("/:id", deleteTask)

		// An internal diagnostics endpoint. It is genuine code, but it should not
		// be part of the public API surface — the custom transform drops it.
		tasks.GET("/_debug", debugTasks)
	}
}

// createTask handles POST /tasks: bind a CreateTaskRequest, return the new Task.
func createTask(c *gin.Context) {
	var req CreateTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, ErrorResponse{Message: "invalid request body", Code: "bad_request"})
		return
	}
	task := Task{Title: req.Title, Status: req.Status, Priority: req.Priority, DueAt: req.DueAt, Assignee: req.Assignee, Labels: req.Labels}
	c.JSON(http.StatusCreated, task)
}

// listTasks handles GET /tasks with an optional ?status= filter.
func listTasks(c *gin.Context) {
	status := c.Query("status")
	_ = status
	c.JSON(http.StatusOK, TaskList{Tasks: []Task{}})
}

// getTask handles GET /tasks/:id.
func getTask(c *gin.Context) {
	id := c.Param("id")
	if id == "" {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "task not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Task{ID: id})
}

// updateTask handles PUT /tasks/:id: bind an UpdateTaskRequest, return the Task.
func updateTask(c *gin.Context) {
	id := c.Param("id")
	var req UpdateTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusNotFound, ErrorResponse{Message: "task not found", Code: "not_found"})
		return
	}
	c.JSON(http.StatusOK, Task{ID: id})
}

// deleteTask handles DELETE /tasks/:id.
func deleteTask(c *gin.Context) {
	id := c.Param("id")
	c.JSON(http.StatusOK, ErrorResponse{Message: "deleted " + id, Code: "ok"})
}

// debugTasks handles the internal GET /tasks/_debug endpoint. The .gnr8/ custom
// transform removes this route before generation, so it is absent from the
// generated OpenAPI + SDK.
func debugTasks(c *gin.Context) {
	c.JSON(http.StatusOK, TaskList{Tasks: []Task{}})
}
