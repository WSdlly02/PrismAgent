package model

import (
	"context"
	"fmt"
	"os"
	"strings"

	openai "github.com/openai/openai-go"
	"github.com/openai/openai-go/option"
	"github.com/openai/openai-go/shared"
)

const (
	DeepSeekAPIKeyEnv     = "DEEPSEEK_API_KEY"
	DeepSeekAPIBaseURLEnv = "DEEPSEEK_API_BASE_URL"
	DeepSeekAPIModelEnv   = "DEEPSEEK_API_MODEL"
)

type DeepSeekConfig struct {
	APIKey  string
	BaseURL string
	Model   string
}

type DeepSeekClient struct {
	client openai.Client
	model  string
}

func DeepSeekConfigFromEnv() (DeepSeekConfig, bool) {
	cfg := DeepSeekConfig{
		APIKey:  strings.TrimSpace(os.Getenv(DeepSeekAPIKeyEnv)),
		BaseURL: strings.TrimSpace(os.Getenv(DeepSeekAPIBaseURLEnv)),
		Model:   strings.TrimSpace(os.Getenv(DeepSeekAPIModelEnv)),
	}
	return cfg, cfg.APIKey != "" && cfg.BaseURL != "" && cfg.Model != ""
}

func NewDeepSeekClient(cfg DeepSeekConfig) (*DeepSeekClient, error) {
	if strings.TrimSpace(cfg.APIKey) == "" {
		return nil, fmt.Errorf("%s is required", DeepSeekAPIKeyEnv)
	}
	if strings.TrimSpace(cfg.BaseURL) == "" {
		return nil, fmt.Errorf("%s is required", DeepSeekAPIBaseURLEnv)
	}
	if strings.TrimSpace(cfg.Model) == "" {
		return nil, fmt.Errorf("%s is required", DeepSeekAPIModelEnv)
	}
	client := openai.NewClient(
		option.WithAPIKey(cfg.APIKey),
		option.WithBaseURL(cfg.BaseURL),
	)
	return &DeepSeekClient{
		client: client,
		model:  cfg.Model,
	}, nil
}

func (c *DeepSeekClient) Complete(ctx context.Context, req Request) (Response, error) {
	messages := make([]openai.ChatCompletionMessageParamUnion, 0, len(req.Messages))
	for _, message := range req.Messages {
		switch message.Role {
		case "system":
			messages = append(messages, openai.SystemMessage(message.Content))
		case "assistant":
			messages = append(messages, openai.AssistantMessage(message.Content))
		case "user":
			messages = append(messages, openai.UserMessage(message.Content))
		default:
			messages = append(messages, openai.UserMessage(message.Content))
		}
	}

	modelName := c.model
	if strings.TrimSpace(req.Model) != "" && req.Model != "default" {
		modelName = req.Model
	}
	completion, err := c.client.Chat.Completions.New(ctx, openai.ChatCompletionNewParams{
		Messages: messages,
		Model:    shared.ChatModel(modelName),
	})
	if err != nil {
		return Response{}, err
	}
	if len(completion.Choices) == 0 {
		return Response{}, fmt.Errorf("deepseek chat completion returned no choices")
	}
	choice := completion.Choices[0]
	return Response{
		Text:  choice.Message.Content,
		Model: completion.Model,
		Usage: Usage{
			InputTokens:  int(completion.Usage.PromptTokens),
			OutputTokens: int(completion.Usage.CompletionTokens),
			TotalTokens:  int(completion.Usage.TotalTokens),
		},
		RawPayload:   []byte(completion.RawJSON()),
		FinishReason: string(choice.FinishReason),
	}, nil
}
