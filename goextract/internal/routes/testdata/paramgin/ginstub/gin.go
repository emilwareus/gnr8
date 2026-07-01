package gin

type Context struct{}
type HandlerFunc func(*Context)
type Engine struct{}
type RouterGroup struct{}

func (e *Engine) Group(string) *RouterGroup       { return &RouterGroup{} }
func (g *RouterGroup) Group(string) *RouterGroup  { return &RouterGroup{} }
func (g *RouterGroup) GET(string, HandlerFunc)    {}
func (g *RouterGroup) POST(string, HandlerFunc)   {}
func (g *RouterGroup) PUT(string, HandlerFunc)    {}
func (g *RouterGroup) DELETE(string, HandlerFunc) {}
