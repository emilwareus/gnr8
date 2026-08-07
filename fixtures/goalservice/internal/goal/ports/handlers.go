package ports

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"

	"github.com/gnr8/gnr8-fixtures/goalservice/internal/common/dto"
)

// createGoal creates a goal for the calling actor.
//
// The goal starts in the pending state and is assigned a server-generated
// identifier, which is returned in the response.
func (h HttpServer) createGoal(c *gin.Context) {
	var input dto.CreateGoalInput
	if err := c.ShouldBindJSON(&input); err != nil {
		c.JSON(http.StatusBadRequest, dto.HttpError{
			Message: "invalid goal payload",
			Slug:    "goal-invalid-input",
			Hints:   []string{"name is required", "analyticsQuery is required"},
		})
		return
	}

	// No real persistence — the fixture only needs to exercise the binding and
	// response shapes so the analyzer has facts to extract.
	c.JSON(http.StatusCreated, dto.CommandMessageWithUUID{
		CommandMessage: dto.CommandMessage{Message: "goal created"},
		UUID:           uuid.New(),
	})
}

// listGoals returns a page of goals for the calling actor.
//
// Results are ordered newest first and paginated with an opaque cursor. Pass the
// cursor from the previous page to continue; omit it to start from the beginning.
func (h HttpServer) listGoals(c *gin.Context) {
	// Fixture note, deliberately INSIDE the body rather than in the doc comment:
	// these params are read loosely via c.Query with no binding struct, so their
	// type and required-ness are not expressible in code. That is the "param type
	// unknown" diagnostic trigger (TARGET-API.md §5.4). Keeping this here proves
	// that only the doc comment becomes API prose — body comments never do.
	cursor := c.Query("cursor")
	pageSize := c.Query("page_size")      // untyped query param
	aggregation := c.Query("aggregation") // untyped query param

	// The values are only echoed into the response shape; no real query runs.
	_ = cursor
	_ = pageSize
	_ = aggregation

	c.JSON(http.StatusOK, dto.ListGoalsOutput{
		Goals:    []dto.GoalResponse{},
		PageSize: 20,
		Total:    0,
	})
}

// updateGoal replaces the mutable fields of one goal.
//
// The goal is identified by its uuid. Fields omitted from the payload are left
// unchanged; the goal's identifier and creation time are never modified.
func (h HttpServer) updateGoal(c *gin.Context) {
	id := c.Param("uuid") // path param :uuid -> {uuid}
	if _, err := uuid.Parse(id); err != nil {
		c.JSON(http.StatusBadRequest, dto.HttpError{
			Message: "invalid goal uuid",
			Slug:    "goal-invalid-uuid",
		})
		return
	}

	var input dto.UpdateGoalInput
	if err := c.ShouldBindJSON(&input); err != nil {
		c.JSON(http.StatusBadRequest, dto.HttpError{
			Message: "invalid goal payload",
			Slug:    "goal-invalid-input",
		})
		return
	}

	// A real handler would 404 when the goal does not exist; the fixture always
	// reports success so both the 200 and the annotated 404 shapes are present.
	c.JSON(http.StatusOK, dto.CommandMessage{Message: "goal updated"})
}

// deleteGoal permanently removes one goal.
func (h HttpServer) deleteGoal(c *gin.Context) {
	id := c.Param("uuid") // path param :uuid -> {uuid}
	if _, err := uuid.Parse(id); err != nil {
		c.JSON(http.StatusBadRequest, dto.HttpError{
			Message: "invalid goal uuid",
			Slug:    "goal-invalid-uuid",
		})
		return
	}

	c.JSON(http.StatusOK, dto.CommandMessage{Message: "goal deleted"})
}

// ensure time is referenced so a future handler can stamp responses; the fixture
// keeps the import live without persistence. (GoalResponse.CreatedAt uses
// time.Time in the DTO package.)
var _ = time.Now
