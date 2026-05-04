package model

import (
	"context"
	"fmt"
	"strings"
)

type MockClient struct{}

func (MockClient) Complete(_ context.Context, req Request) (Response, error) {
	var user string
	for i := len(req.Messages) - 1; i >= 0; i-- {
		if req.Messages[i].Role == "user" {
			user = req.Messages[i].Content
			break
		}
	}
	if strings.TrimSpace(user) == "" {
		user = "No user message provided."
	}
	if strings.HasPrefix(user, "User message:\n") {
		user = strings.TrimPrefix(user, "User message:\n")
		if idx := strings.Index(user, "\n\nLocal workspace context:"); idx >= 0 {
			user = user[:idx]
		}
	}
	text := fmt.Sprintf("Mock agent response.\n\nI received your message:\n\n%s\n\nA real ModelClient adapter will replace this response.", user)
	return Response{
		Text:         text,
		Model:        "mock",
		FinishReason: "stop",
	}, nil
}
