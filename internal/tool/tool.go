package tool

import "context"

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
