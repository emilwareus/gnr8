// EXPECTED SDK SHAPE — acceptance target, not compiled; gnr8 must generate
// equivalents in Phase 3 (D-05).
//
// This sketches the tag-grouped typed operation methods for the "Goals" tag
// (CONTEXT D-05). Every operation takes context.Context as its FIRST argument
// (idiomatic Go — NOT the verbose openapi-generator builder pattern). It is
// illustrative — it is NOT part of the Go build and need not compile.
package goalservice

import "context"

// CreateGoal -> POST /goal/ : body CreateGoalInput, success 201 CommandMessageWithUUID.
func (c *Client) CreateGoal(ctx context.Context, in CreateGoalInput) (CommandMessageWithUUID, error) {
	// real impl: marshal in -> POST baseURL+"/goal/" -> decode 201 body / typed APIError on 4xx
	var out CommandMessageWithUUID
	return out, nil
}

// ListGoalsParams carries the query parameters for ListGoals (cursor/page_size
// and the required aggregation enum). Optional params are pointers.
type ListGoalsParams struct {
	Cursor      *string
	PageSize    *string
	Aggregation string // required; closed value set: count|sum|avg|min|max
}

// ListGoals -> GET /goal/list : query params, success 200 ListGoalsOutput.
func (c *Client) ListGoals(ctx context.Context, params ListGoalsParams) (ListGoalsOutput, error) {
	var out ListGoalsOutput
	return out, nil
}

// UpdateGoal -> PUT /goal/{uuid} : path param uuid + body UpdateGoalInput,
// success 200 CommandMessage (404/400 -> typed APIError).
func (c *Client) UpdateGoal(ctx context.Context, uuid string, in UpdateGoalInput) (CommandMessage, error) {
	var out CommandMessage
	return out, nil
}

// DeleteGoal -> DELETE /goal/{uuid} : path param uuid, success 200 CommandMessage.
func (c *Client) DeleteGoal(ctx context.Context, uuid string) (CommandMessage, error) {
	var out CommandMessage
	return out, nil
}
