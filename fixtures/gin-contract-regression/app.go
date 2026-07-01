package gincontract

import (
	"net/http"

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

func RegisterRoutes(r *gin.Engine, h *Handler) {
	v1 := r.Group("/v1")

	auth := v1.Group("/auth")
	auth.POST("/login", h.login)
	auth.POST("/logout", h.logout)

	files := v1.Group("/files")
	files.GET("/:fileId/download", h.downloadFile)
	files.GET("/:fileId/open", h.openFile)
	files.GET("/:fileId/stream", h.streamFile)

	items := v1.Group("/items")
	items.GET("/:itemId/children/:childId", h.getChild)
	items.PATCH("/:itemId", h.updateItem)
	items.GET("/saved-views", h.listSavedViews)
	items.POST("/jobs", h.createJob)
	items.DELETE("/:itemId", h.deleteItem)
}

func (h *Handler) login(c *gin.Context) {
	var body LoginRequest
	if err := c.ShouldBindJSON(&body); err != nil {
		c.JSON(http.StatusBadRequest, MessageResponse{Message: err.Error()})
		return
	}
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
	c.Data(http.StatusOK, "application/pdf", []byte("..."))
}

func (h *Handler) createJob(c *gin.Context) {
	c.JSON(http.StatusAccepted, gin.H{
		"jobId": "job_123",
	})
}

func parsePathUUID(c *gin.Context, name string) (string, bool) {
	value := c.Param(name)
	return value, value != ""
}
