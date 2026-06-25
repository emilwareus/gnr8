package sdk

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
)

// ListBooksParams carries the query parameters for ListBooks.
type ListBooksParams struct {
	Genre *string
}

// ListBooks -> GET /books/
func (c *Client) ListBooks(ctx context.Context, params ListBooksParams) (BookList, error) {
	var out BookList
	reqURL := c.baseURL + "/books/"
	req, err := http.NewRequestWithContext(ctx, "GET", reqURL, nil)
	if err != nil {
		return out, err
	}
	q := req.URL.Query()
	if params.Genre != nil {
		q.Set("genre", *params.Genre)
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

// CreateBook -> POST /books/
func (c *Client) CreateBook(ctx context.Context, in CreateBookRequest) (Book, error) {
	var out Book
	payload, err := json.Marshal(in)
	if err != nil {
		return out, err
	}
	reqBody := bytes.NewReader(payload)
	reqURL := c.baseURL + "/books/"
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
		var apiErr ErrorResponse
		_ = json.NewDecoder(resp.Body).Decode(&apiErr)
		return out, &APIError{
			StatusCode: resp.StatusCode,
			Message:    apiErr.Message,
		}
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return out, err
	}
	return out, nil
}

// DeleteBook -> DELETE /books/{id}
func (c *Client) DeleteBook(ctx context.Context, id string) (ErrorResponse, error) {
	var out ErrorResponse
	reqURL := c.baseURL + fmt.Sprintf("/books/%s", url.PathEscape(id))
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

// GetBook -> GET /books/{id}
func (c *Client) GetBook(ctx context.Context, id string) (Book, error) {
	var out Book
	reqURL := c.baseURL + fmt.Sprintf("/books/%s", url.PathEscape(id))
	req, err := http.NewRequestWithContext(ctx, "GET", reqURL, nil)
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
		var apiErr ErrorResponse
		_ = json.NewDecoder(resp.Body).Decode(&apiErr)
		return out, &APIError{
			StatusCode: resp.StatusCode,
			Message:    apiErr.Message,
		}
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return out, err
	}
	return out, nil
}

// UpdateBook -> PUT /books/{id}
func (c *Client) UpdateBook(ctx context.Context, id string, in UpdateBookRequest) (Book, error) {
	var out Book
	payload, err := json.Marshal(in)
	if err != nil {
		return out, err
	}
	reqBody := bytes.NewReader(payload)
	reqURL := c.baseURL + fmt.Sprintf("/books/%s", url.PathEscape(id))
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
		var apiErr ErrorResponse
		_ = json.NewDecoder(resp.Body).Decode(&apiErr)
		return out, &APIError{
			StatusCode: resp.StatusCode,
			Message:    apiErr.Message,
		}
	}
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return out, err
	}
	return out, nil
}
