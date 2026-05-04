package tool

import (
	"context"
	"fmt"
)

type Request struct {
	Name     string
	Args     map[string]string
	RawInput string
}

type Result struct {
	RawOutput   string
	Summary     string
	ArtifactRef string
	Metadata    map[string]string
}

type Tool interface {
	Name() string
	Call(ctx context.Context, req Request) (Result, error)
}

type Registry struct {
	tools map[string]Tool
}

func NewRegistry(tools ...Tool) *Registry {
	registry := &Registry{tools: make(map[string]Tool)}
	for _, tool := range tools {
		registry.Register(tool)
	}
	return registry
}

func (r *Registry) Register(tool Tool) {
	if tool == nil {
		return
	}
	r.tools[tool.Name()] = tool
}

func (r *Registry) Call(ctx context.Context, req Request) (Result, error) {
	tool, ok := r.tools[req.Name]
	if !ok {
		return Result{}, fmt.Errorf("tool not found: %s", req.Name)
	}
	return tool.Call(ctx, req)
}

func (r *Registry) Names() []string {
	names := make([]string, 0, len(r.tools))
	for name := range r.tools {
		names = append(names, name)
	}
	return names
}
