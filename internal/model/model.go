package model

import "context"

type Message struct {
	Role             string
	Content          string
	ReasoningContent string
	ToolCallID       string
	ToolCalls        []ToolCall
}

type Request struct {
	Model    string
	Messages []Message
	Tools    []ToolDefinition
	Metadata map[string]string
}

type Response struct {
	Text             string
	ReasoningContent string
	Model            string
	Usage            Usage
	RawPayload       []byte
	FinishReason     string
	ToolCalls        []ToolCall
}

type Usage struct {
	InputTokens  int
	OutputTokens int
	TotalTokens  int
}

type Client interface {
	Complete(ctx context.Context, req Request) (Response, error)
}

type ToolDefinition struct {
	Name        string
	Description string
	Parameters  map[string]any
}

type ToolCall struct {
	ID           string
	Name         string
	Arguments    map[string]string
	RawArguments string
}
