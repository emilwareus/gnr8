// EXPECTED SDK SHAPE — acceptance target, not compiled; gnr8 must generate
// equivalents in Phase 3 (D-05).
//
// Generated request/response model structs, mapped per the TARGET-API.md §4 type
// table. Note the SDK-side type choices:
//   - uuid.UUID -> string         (well-known mapping)
//   - time.Time -> time.Time      (well-known mapping)
//   - *float64  -> *float32        (⚠ generator narrows number->float32; see
//     diagnostics.txt — gnr8 may instead keep
//     float64 and warn)
//   - map[string]any -> map[string]any (free-form; additionalProperties: true)
//
// It is illustrative — it is NOT part of the Go build and need not compile.
package goalservice

import "time"

// TargetDirection is the string-enum newtype (gte|lte).
type TargetDirection string

const (
	TargetDirectionGte TargetDirection = "gte"
	TargetDirectionLte TargetDirection = "lte"
)

// GoalAnalyticsQuery is the nested model ($ref in OpenAPI).
type GoalAnalyticsQuery struct {
	Metric      string `json:"metric"`
	WindowDays  int64  `json:"windowDays,omitempty"`
	IncludePast bool   `json:"includePast,omitempty"`
}

// CreateGoalInput is the POST /goal/ request model.
type CreateGoalInput struct {
	Name             string             `json:"name"`
	Description      string             `json:"description,omitempty"`
	AnalyticsQuery   GoalAnalyticsQuery `json:"analyticsQuery"`
	TargetValue      *float32           `json:"targetValue,omitempty"` // ⚠ narrowed from float64
	TargetDirection  *TargetDirection   `json:"targetDirection,omitempty"`
	WorkflowChainIDs []string           `json:"workflowChainIds,omitempty"` // []uuid.UUID -> []string
}

// UpdateGoalInput is the PUT /goal/{uuid} request model.
type UpdateGoalInput struct {
	Name             string              `json:"name,omitempty"`
	Description      string              `json:"description,omitempty"`
	AnalyticsQuery   *GoalAnalyticsQuery `json:"analyticsQuery,omitempty"`
	TargetValue      *float32            `json:"targetValue,omitempty"` // ⚠ narrowed from float64
	TargetDirection  *TargetDirection    `json:"targetDirection,omitempty"`
	WorkflowChainIDs []string            `json:"workflowChainIds,omitempty"`
}

// GoalResponse is the goal read model.
type GoalResponse struct {
	UUID            string             `json:"uuid"` // uuid.UUID -> string
	Name            string             `json:"name"`
	Description     string             `json:"description,omitempty"`
	AnalyticsQuery  GoalAnalyticsQuery `json:"analyticsQuery"`
	TargetValue     *float32           `json:"targetValue,omitempty"`
	TargetDirection *TargetDirection   `json:"targetDirection,omitempty"`
	CreatedAt       time.Time          `json:"createdAt"`          // time.Time -> time.Time
	Metadata        map[string]any     `json:"metadata,omitempty"` // free-form map
}

// ListGoalsOutput is the GET /goal/list response model.
type ListGoalsOutput struct {
	Goals      []GoalResponse `json:"goals"`
	NextCursor *string        `json:"nextCursor,omitempty"`
	PageSize   int64          `json:"pageSize"`
	Total      int64          `json:"total"`
}

// HttpError is the error envelope model.
type HttpError struct {
	Message string   `json:"message"`
	Slug    string   `json:"slug,omitempty"`
	Hints   []string `json:"hints,omitempty"`
}

// CommandMessage is a minimal success envelope.
type CommandMessage struct {
	Message string `json:"message"`
}

// CommandMessageWithUUID flattens CommandMessage's "message" plus a uuid string.
type CommandMessageWithUUID struct {
	Message string `json:"message"`
	UUID    string `json:"uuid"`
}
