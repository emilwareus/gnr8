package goalservice

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
)

// CreateGoal -> POST /goal/
func (c *Client) CreateGoal(ctx context.Context, in CreateGoalInput) (CommandMessageWithUUID, error) {
	var out CommandMessageWithUUID
	payload, err := json.Marshal(in)
	if err != nil {
		return out, err
	}
	reqBody := bytes.NewReader(payload)
	reqURL := c.baseURL + "/goal/"
	req, err := http.NewRequestWithContext(ctx, "POST", reqURL, reqBody)
	if err != nil {
		return out, err
	}
	req.Header.Set("Content-Type", "application/json")
	if c.apiKey != "" {
		req.Header.Set("X-API-Key", c.apiKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return out, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 201 {
		var apiErr HttpError
		_ = json.NewDecoder(resp.Body).Decode(&apiErr)
		return out, &APIError{
			StatusCode: resp.StatusCode,
			Message:    apiErr.Message,
			Slug:       apiErr.Slug,
			Hints:      apiErr.Hints,
		}
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return out, err
	}
	return out, nil
}

// ListGoalsParams carries the query parameters for ListGoals.
type ListGoalsParams struct {
	Aggregation *string
	Cursor      *string
	PageSize    *string
}

// ListGoals -> GET /goal/list
func (c *Client) ListGoals(ctx context.Context, params ListGoalsParams) (ListGoalsOutput, error) {
	var out ListGoalsOutput
	reqURL := c.baseURL + "/goal/list"
	req, err := http.NewRequestWithContext(ctx, "GET", reqURL, nil)
	if err != nil {
		return out, err
	}
	q := req.URL.Query()
	if params.Aggregation != nil {
		q.Set("aggregation", *params.Aggregation)
	}
	if params.Cursor != nil {
		q.Set("cursor", *params.Cursor)
	}
	if params.PageSize != nil {
		q.Set("page_size", *params.PageSize)
	}
	req.URL.RawQuery = q.Encode()
	if c.apiKey != "" {
		req.Header.Set("X-API-Key", c.apiKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return out, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		var apiErr struct {
			Message string   `json:"message"`
			Slug    string   `json:"slug"`
			Hints   []string `json:"hints"`
		}
		_ = json.NewDecoder(resp.Body).Decode(&apiErr)
		return out, &APIError{
			StatusCode: resp.StatusCode,
			Message:    apiErr.Message,
			Slug:       apiErr.Slug,
			Hints:      apiErr.Hints,
		}
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return out, err
	}
	return out, nil
}

// DeleteGoal -> DELETE /goal/{uuid}
func (c *Client) DeleteGoal(ctx context.Context, uuid string) (CommandMessage, error) {
	var out CommandMessage
	reqURL := c.baseURL + fmt.Sprintf("/goal/%s", url.PathEscape(uuid))
	req, err := http.NewRequestWithContext(ctx, "DELETE", reqURL, nil)
	if err != nil {
		return out, err
	}
	if c.apiKey != "" {
		req.Header.Set("X-API-Key", c.apiKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return out, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		var apiErr HttpError
		_ = json.NewDecoder(resp.Body).Decode(&apiErr)
		return out, &APIError{
			StatusCode: resp.StatusCode,
			Message:    apiErr.Message,
			Slug:       apiErr.Slug,
			Hints:      apiErr.Hints,
		}
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return out, err
	}
	return out, nil
}

// UpdateGoal -> PUT /goal/{uuid}
func (c *Client) UpdateGoal(ctx context.Context, uuid string, in UpdateGoalInput) (CommandMessage, error) {
	var out CommandMessage
	payload, err := json.Marshal(in)
	if err != nil {
		return out, err
	}
	reqBody := bytes.NewReader(payload)
	reqURL := c.baseURL + fmt.Sprintf("/goal/%s", url.PathEscape(uuid))
	req, err := http.NewRequestWithContext(ctx, "PUT", reqURL, reqBody)
	if err != nil {
		return out, err
	}
	req.Header.Set("Content-Type", "application/json")
	if c.apiKey != "" {
		req.Header.Set("X-API-Key", c.apiKey)
	}
	resp, err := c.httpClient.Do(req)
	if err != nil {
		return out, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		var apiErr HttpError
		_ = json.NewDecoder(resp.Body).Decode(&apiErr)
		return out, &APIError{
			StatusCode: resp.StatusCode,
			Message:    apiErr.Message,
			Slug:       apiErr.Slug,
			Hints:      apiErr.Hints,
		}
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return out, err
	}
	return out, nil
}
