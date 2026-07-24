package goalservice

import "time"

type CommandMessage struct {
	Message string `json:"message"`
}

type CommandMessageWithUUID struct {
	Message string `json:"message"`
	UUID    string `json:"uuid"`
}

type CreateGoalInput struct {
	AnalyticsQuery   GoalAnalyticsQuery `json:"analyticsQuery"`
	Description      string             `json:"description"`
	Name             string             `json:"name"`
	TargetDirection  *TargetDirection   `json:"targetDirection,omitempty"`
	TargetValue      *float64           `json:"targetValue,omitempty"`
	WorkflowChainIDs []string           `json:"workflowChainIds,omitempty"`
}

type GoalAnalyticsQuery struct {
	IncludePast *bool  `json:"includePast,omitempty"`
	Metric      string `json:"metric"`
	WindowDays  int64  `json:"windowDays"`
}

type GoalResponse struct {
	AnalyticsQuery  GoalAnalyticsQuery `json:"analyticsQuery"`
	CreatedAt       time.Time          `json:"createdAt"`
	Description     string             `json:"description,omitempty"`
	Metadata        map[string]any     `json:"metadata,omitempty"`
	Name            string             `json:"name"`
	TargetDirection *TargetDirection   `json:"targetDirection,omitempty"`
	TargetValue     *float64           `json:"targetValue,omitempty"`
	UUID            string             `json:"uuid"`
}

type HttpError struct {
	Hints   []string `json:"hints,omitempty"`
	Message string   `json:"message"`
	Slug    string   `json:"slug,omitempty"`
}

type ListGoalsOutput struct {
	Goals      []GoalResponse `json:"goals"`
	NextCursor string         `json:"nextCursor,omitempty"`
	PageSize   int64          `json:"pageSize"`
	Total      int64          `json:"total"`
}

type TargetDirection string

const (
	TargetDirectionGte TargetDirection = "gte"
	TargetDirectionLte TargetDirection = "lte"
)

type UpdateGoalInput struct {
	AnalyticsQuery   *GoalAnalyticsQuery `json:"analyticsQuery,omitempty"`
	Description      string              `json:"description,omitempty"`
	Name             string              `json:"name,omitempty"`
	TargetDirection  *TargetDirection    `json:"targetDirection,omitempty"`
	TargetValue      *float64            `json:"targetValue,omitempty"`
	WorkflowChainIDs []string            `json:"workflowChainIds,omitempty"`
}
