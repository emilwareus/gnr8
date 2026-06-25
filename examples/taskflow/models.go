package main

import "time"

// Status is a code-defined string enum for a task's lifecycle. gnr8 reads the
// `const` set below straight from go/types and emits it as an OpenAPI string
// enum (and a typed Go newtype in the SDK) — no annotation needed.
type Status string

const (
	StatusOpen       Status = "open"
	StatusInProgress Status = "in_progress"
	StatusDone       Status = "done"
)

// Assignee is a nested DTO referenced by Task. Because it carries json tags it
// becomes its own component schema and Task.assignee resolves to a $ref.
type Assignee struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Email string `json:"email"`
}

// Task is the core resource. Each field maps cleanly to an OpenAPI schema: the
// Status enum, an int priority, a time.Time due date, an optional *string notes
// (omitempty), a nested Assignee ($ref), and a []string of labels.
type Task struct {
	ID       string    `json:"id"`
	Title    string    `json:"title"`
	Status   Status    `json:"status"`
	Priority int       `json:"priority"`
	DueAt    time.Time `json:"dueAt"`
	Notes    *string   `json:"notes,omitempty"`
	Assignee Assignee  `json:"assignee"`
	Labels   []string  `json:"labels"`
}

// CreateTaskRequest is the POST /tasks body. `binding:"required"` marks the
// required fields gnr8 surfaces in the OpenAPI `required` list.
type CreateTaskRequest struct {
	Title    string    `json:"title" binding:"required"`
	Status   Status    `json:"status" binding:"required"`
	Priority int       `json:"priority"`
	DueAt    time.Time `json:"dueAt"`
	Notes    *string   `json:"notes,omitempty"`
	Assignee Assignee  `json:"assignee"`
	Labels   []string  `json:"labels"`
}

// UpdateTaskRequest is the PUT /tasks/:id body. Every field is an optional update.
type UpdateTaskRequest struct {
	Title    *string  `json:"title,omitempty"`
	Status   *Status  `json:"status,omitempty"`
	Priority *int     `json:"priority,omitempty"`
	Notes    *string  `json:"notes,omitempty"`
	Labels   []string `json:"labels,omitempty"`
}

// TaskList is the GET /tasks response envelope.
type TaskList struct {
	Tasks []Task `json:"tasks"`
}

// ErrorResponse is the error envelope returned for 400 / 404 responses.
type ErrorResponse struct {
	Message string `json:"message"`
	Code    string `json:"code"`
}
