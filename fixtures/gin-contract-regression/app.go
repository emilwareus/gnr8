package gincontract

import (
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
)

type Handler struct{}

type LoginRequest struct {
	Email string `json:"email" binding:"required"`
}

type LoginResponse struct {
	Token string `json:"token"`
}

type UpdateItemRequest struct {
	Name string `json:"name"`
}

type ItemResponse struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type ChildResponse struct {
	ItemID  string `json:"itemId"`
	ChildID string `json:"childId"`
}

type SavedViewResponse struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type MessageResponse struct {
	Message string `json:"message"`
}

type SearchResponse struct {
	Q      string `json:"q"`
	Limit  int    `json:"limit"`
	Days   int    `json:"days"`
	Sort   string `json:"sort"`
	Cursor string `json:"cursor"`
	Token  string `json:"token"`
}

type MarkReadRequest struct {
	LastID string `json:"lastId"`
}

type FileBytes []byte

const (
	defaultSort   = "asc"
	defaultCursor = "first"
	defaultDays   = 5
)

func RegisterRoutes(r *gin.Engine, h *Handler) {
	v1 := r.Group("/v1")

	auth := v1.Group("/auth")
	auth.POST("/login", h.login)
	auth.POST("/logout", h.logout)

	files := v1.Group("/files")
	files.GET("/:fileId/download", h.downloadFile)
	files.GET("/:fileId/open", h.openFile)
	files.GET("/:fileId/stream", h.streamFile)
	files.GET("/:fileId/read", h.readFile)

	items := v1.Group("/items")
	items.GET("/:itemId/children/:childId", h.getChild)
	items.PATCH("/:itemId", h.updateItem)
	items.PATCH("/:itemId/read", h.markRead)
	items.PATCH("/:itemId/header-read", h.headerRead)
	items.PATCH("/:itemId/combined-header-read", h.combinedHeaderRead)
	items.PATCH("/:itemId/force-read", h.forceRead)
	items.PATCH("/:itemId/mixed-read", h.mixedRead)
	items.PATCH("/:itemId/unrelated-length-read", h.unrelatedLengthRead)
	items.GET("/saved-views", h.listSavedViews)
	items.GET("/search", h.searchItems)
	items.GET("/attendance", h.attendance)
	items.GET("/events", h.itemEvents)
	items.GET("/raw-stream", h.rawStream)
	items.POST("/jobs", h.createJob)
	items.DELETE("/:itemId", h.deleteItem)
}

func (h *Handler) login(c *gin.Context) {
	var body LoginRequest
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
		return
	}
	c.Status(http.StatusOK)
	c.JSON(http.StatusOK, LoginResponse{Token: "token"})
}

func (h *Handler) logout(c *gin.Context) {
	c.Status(http.StatusNoContent)
}

func (h *Handler) getChild(c *gin.Context) {
	itemID, ok := parsePathUUID(c, "itemId")
	if !ok {
		return
	}
	childID := c.Param("childId")
	c.JSON(http.StatusOK, ChildResponse{ItemID: itemID, ChildID: childID})
}

func (h *Handler) updateItem(c *gin.Context) {
	var body UpdateItemRequest
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
		return
	}
	c.JSON(http.StatusOK, ItemResponse{ID: c.Param("itemId"), Name: body.Name})
}

func (h *Handler) deleteItem(c *gin.Context) {
	c.AbortWithStatus(http.StatusNoContent)
}

func (h *Handler) listSavedViews(c *gin.Context) {
	views := []SavedViewResponse{
		{ID: "1", Name: "Default"},
	}
	c.JSON(http.StatusOK, views)
}

func (h *Handler) downloadFile(c *gin.Context) {
	c.Header("Content-Type", "application/octet-stream")
	c.FileAttachment("/tmp/report.pdf", "report.pdf")
}

func (h *Handler) openFile(c *gin.Context) {
	c.File("/tmp/report.pdf")
}

func (h *Handler) streamFile(c *gin.Context) {
	payload := FileBytes("...")
	c.Data(http.StatusOK, "application/pdf", payload)
}

func (h *Handler) readFile(c *gin.Context) {
	c.DataFromReader(http.StatusOK, 12, attachmentContentType(), strings.NewReader("hello"), nil)
}

func (h *Handler) itemEvents(c *gin.Context) {
	c.Stream(func(w io.Writer) bool {
		c.SSEvent("message", MessageResponse{Message: "ok"})
		return false
	})
}

func (h *Handler) rawStream(c *gin.Context) {
	c.Stream(func(w io.Writer) bool {
		return false
	})
}

func (h *Handler) createJob(c *gin.Context) {
	c.JSON(http.StatusAccepted, gin.H{
		"jobId": "job_123",
	})
}

func (h *Handler) searchItems(c *gin.Context) {
	q := strings.TrimSpace(c.Query("q"))
	limit := parseOptionalPositiveInt(c.Query("limit"))
	trimmedLimit := parseOptionalPositiveInt(strings.TrimSpace(c.Query("trimmedLimit")))
	wrappedLimit := fmt.Sprint(parseOptionalPositiveInt(c.Query("wrappedLimit")))
	sort := parseSort(strings.TrimSpace(c.Query("sort")), defaultSort)
	cursor := parseSort(c.DefaultQuery("cursor", defaultCursor), "fallback")
	token, _ := c.GetQuery("token")
	_ = wrappedLimit
	_ = trimmedLimit
	c.JSON(http.StatusOK, SearchResponse{Q: q, Limit: limit, Sort: sort, Cursor: cursor, Token: token})
}

func (h *Handler) attendance(c *gin.Context) {
	startDate, err := parseAttendanceTime(c.Query("startDate"), "bad-date", "invalid")
	if err != nil {
		c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
		return
	}
	days, err := parseAttendanceDays(c.Query("days"), defaultDays, 31)
	if err != nil {
		c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
		return
	}
	_ = startDate
	c.JSON(http.StatusOK, SearchResponse{Days: days})
}

func (h *Handler) markRead(c *gin.Context) {
	var body MarkReadRequest
	if len(strings.TrimSpace(c.GetHeader("Content-Length"))) != 0 || c.Request.ContentLength > 0 {
		if err := c.ShouldBindJSON(&body); err != nil {
			c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
			return
		}
	}
	c.JSON(http.StatusOK, MessageResponse{Message: body.LastID})
}

func (h *Handler) headerRead(c *gin.Context) {
	var body MarkReadRequest
	if c.GetHeader("Content-Length") != "" {
		if err := c.ShouldBindJSON(&body); err != nil {
			c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
			return
		}
	}
	c.JSON(http.StatusOK, MessageResponse{Message: body.LastID})
}

func (h *Handler) combinedHeaderRead(c *gin.Context) {
	var body MarkReadRequest
	if len(c.GetHeader("Content-Length")+c.GetHeader("X-Force-Bind")) != 0 {
		if err := c.ShouldBindJSON(&body); err != nil {
			c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
			return
		}
	}
	c.JSON(http.StatusOK, MessageResponse{Message: body.LastID})
}

func (h *Handler) forceRead(c *gin.Context) {
	var body MarkReadRequest
	if c.Request.ContentLength > 0 || c.GetHeader("X-Force-Bind") != "" {
		if err := c.ShouldBindJSON(&body); err != nil {
			c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
			return
		}
	}
	c.JSON(http.StatusOK, MessageResponse{Message: body.LastID})
}

func (h *Handler) mixedRead(c *gin.Context) {
	var body MarkReadRequest
	if c.Request.ContentLength > 0 {
		if err := c.ShouldBindJSON(&body); err != nil {
			c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
			return
		}
	}
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
		return
	}
	c.JSON(http.StatusOK, MessageResponse{Message: body.LastID})
}

func (h *Handler) unrelatedLengthRead(c *gin.Context) {
	var body MarkReadRequest
	var request http.Request
	if request.ContentLength > 0 {
		if err := c.ShouldBindJSON(&body); err != nil {
			c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
			return
		}
	}
	c.JSON(http.StatusOK, MessageResponse{Message: body.LastID})
}

func parsePathUUID(c *gin.Context, name string) (string, bool) {
	value := c.Param(name)
	return value, value != ""
}

func parseOptionalPositiveInt(raw string) int {
	if raw == "" {
		return 0
	}
	return 1
}

func parseAttendanceTime(raw string, _, _ string) (time.Time, error) {
	if raw == "" {
		return time.Time{}, nil
	}
	return time.Now(), nil
}

func parseAttendanceDays(raw string, fallback, _ int) (int, error) {
	if raw == "" {
		return fallback, nil
	}
	return fallback + 1, nil
}

func parseSort(raw string, fallback string) string {
	if raw == "" {
		return fallback
	}
	return raw
}

func attachmentContentType() string {
	return "application/pdf"
}
