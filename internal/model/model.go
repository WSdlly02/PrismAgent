package model

import "context"

type Message struct {
	Role    string
	Content string
}

type Request struct {
	Model    string
	Messages []Message
	Metadata map[string]string
}

type Response struct {
	Text         string
	Model        string
	Usage        Usage
	RawPayload   []byte
	FinishReason string
}

type Usage struct {
	InputTokens  int
	OutputTokens int
	TotalTokens  int
}

type Client interface {
	Complete(ctx context.Context, req Request) (Response, error)
}
