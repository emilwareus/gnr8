package ports

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"

	"github.com/gnr8/gnr8-fixtures/goalservice/internal/common/dto"
)

// createGoal handles POST /goal/.
//
// This handler is fully code-inferable:
//   - request body type = dto.CreateGoalInput (from ShouldBindJSON(&input))
//   - success           = 201 dto.CommandMessageWithUUID (c.JSON(StatusCreated, ...))
//   - error             = 400 dto.HttpError              (c.JSON(StatusBadRequest, ...))
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

// listGoals handles GET /goal/list.
//
// Query params are read loosely via c.Query (no binding struct), so their type
// and required-ness aren't expressible in the code. The untyped c.Query reads
// are the "param type unknown" diagnostic trigger (TARGET-API.md §5.4).
//
func (h HttpServer) listGoals(c *gin.Context) {
	cursor := c.Query("cursor")           // untyped query param -> diagnostic trigger
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

// updateGoal handles PUT /goal/{uuid}.
//
// This handler exercises c.Param (path param) and ShouldBindJSON (request body)
// and returns multiple success/failure status codes — all derived from the code.
//
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

// deleteGoal handles DELETE /goal/{uuid}: path param + simple response.
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
