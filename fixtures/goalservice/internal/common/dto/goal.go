package dto

import (
	"time"

	"github.com/google/uuid"
)

// GoalAnalyticsQuery is a nested struct referenced by the goal inputs. A nested
// struct field must lower to a $ref to this struct's schema.
type GoalAnalyticsQuery struct {
	Metric      string `json:"metric" binding:"required" description:"Analytics metric to evaluate"`
	WindowDays  int    `json:"windowDays" description:"Rolling window size in days"`
	IncludePast bool   `json:"includePast,omitempty" description:"Whether to include historical data points"`
}

// CreateGoalInput is the request body for POST /goal/. It is intentionally the
// densest DTO: it covers EVERY FIX-02 schema feature in one place.
//
//   - Name             required string with a description tag
//   - Description       plain string
//   - AnalyticsQuery    nested struct ($ref), required
//   - TargetValue       *float64 + omitempty -> optional, AND the float64->float32
//     precision-loss diagnostic trigger (TARGET-API.md §5.2)
//   - TargetDirection   *TargetDirection -> optional enum
//   - WorkflowChainIDs  []uuid.UUID -> array of a well-known type; note the json
//     name (workflowChainIds) differs from the Go field name (WorkflowChainIDs)
type CreateGoalInput struct {
	Name             string             `json:"name" binding:"required" description:"Short human-readable goal name"`
	Description      string             `json:"description" description:"Longer explanation"`
	AnalyticsQuery   GoalAnalyticsQuery `json:"analyticsQuery" binding:"required"`
	TargetValue      *float64           `json:"targetValue,omitempty"`
	TargetDirection  *TargetDirection   `json:"targetDirection,omitempty"`
	WorkflowChainIDs []uuid.UUID        `json:"workflowChainIds,omitempty"`
}

// UpdateGoalInput is the request body for PUT /goal/{uuid}. Same general shape as
// CreateGoalInput; used by the annotation-described update handler.
type UpdateGoalInput struct {
	Name             string              `json:"name,omitempty" description:"Updated goal name"`
	Description      string              `json:"description,omitempty" description:"Updated explanation"`
	AnalyticsQuery   *GoalAnalyticsQuery `json:"analyticsQuery,omitempty"`
	TargetValue      *float64            `json:"targetValue,omitempty"`
	TargetDirection  *TargetDirection    `json:"targetDirection,omitempty"`
	WorkflowChainIDs []uuid.UUID         `json:"workflowChainIds,omitempty"`
}

// GoalResponse is a single goal as returned to clients. It exercises the
// time.Time and uuid.UUID well-known types AND the UNSUPPORTED free-form map
// pattern (Metadata map[string]any) that must surface as a diagnostic
// (additionalProperties: true) per TARGET-API.md §5.1.
type GoalResponse struct {
	UUID            uuid.UUID          `json:"uuid" binding:"required"`
	Name            string             `json:"name" binding:"required"`
	Description     string             `json:"description,omitempty"`
	AnalyticsQuery  GoalAnalyticsQuery `json:"analyticsQuery"`
	TargetValue     *float64           `json:"targetValue,omitempty"`
	TargetDirection *TargetDirection   `json:"targetDirection,omitempty"`
	CreatedAt       time.Time          `json:"createdAt"`
	// Metadata is a free-form map -> UNSUPPORTED pattern. Lowers to
	// additionalProperties: true and must emit a diagnostic (FIX-02 requirement).
	Metadata map[string]any `json:"metadata,omitempty"`
}

// ListGoalsOutput is the response body for GET /goal/list. It contains an array
// of nested GoalResponse structs plus cursor-pagination fields.
type ListGoalsOutput struct {
	Goals      []GoalResponse `json:"goals"`
	NextCursor *uuid.UUID     `json:"nextCursor,omitempty"`
	PageSize   int            `json:"pageSize"`
	Total      int64          `json:"total"`
}
