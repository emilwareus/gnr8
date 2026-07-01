package gin

type H map[string]any
type HandlerFunc func(*Context)

type Engine struct{}
type RouterGroup struct{}
type Context struct{}

func (e *Engine) Group(string) *RouterGroup { return &RouterGroup{} }

func (g *RouterGroup) Group(string) *RouterGroup  { return &RouterGroup{} }
func (g *RouterGroup) GET(string, HandlerFunc)    {}
func (g *RouterGroup) POST(string, HandlerFunc)   {}
func (g *RouterGroup) PATCH(string, HandlerFunc)  {}
func (g *RouterGroup) DELETE(string, HandlerFunc) {}

func (c *Context) Param(string) string           { return "" }
func (c *Context) ShouldBindJSON(any) error      { return nil }
func (c *Context) JSON(int, any)                 {}
func (c *Context) Status(int)                    {}
func (c *Context) AbortWithStatus(int)           {}
func (c *Context) Header(string, string)         {}
func (c *Context) FileAttachment(string, string) {}
func (c *Context) File(string)                   {}
func (c *Context) Data(int, string, []byte)      {}
