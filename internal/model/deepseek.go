package model

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"strings"

	openai "github.com/openai/openai-go"
	"github.com/openai/openai-go/option"
	"github.com/openai/openai-go/packages/param"
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
			if len(message.ToolCalls) > 0 {
				messages = append(messages, assistantToolCallMessage(message))
			} else {
				messages = append(messages, openai.AssistantMessage(message.Content))
			}
		case "tool":
			messages = append(messages, openai.ToolMessage(message.Content, message.ToolCallID))
		case "user":
			messages = append(messages, openai.UserMessage(message.Content))
		default:
			messages = append(messages, openai.UserMessage(message.Content))
		}
	}
	tools := make([]openai.ChatCompletionToolParam, 0, len(req.Tools))
	for _, definition := range req.Tools {
		tools = append(tools, openai.ChatCompletionToolParam{
			Function: shared.FunctionDefinitionParam{
				Name:        definition.Name,
				Description: openai.String(definition.Description),
				Parameters:  shared.FunctionParameters(definition.Parameters),
			},
		})
	}

	modelName := c.model
	if strings.TrimSpace(req.Model) != "" && req.Model != "default" {
		modelName = req.Model
	}
	params := openai.ChatCompletionNewParams{
		Messages: messages,
		Model:    shared.ChatModel(modelName),
	}
	if len(tools) > 0 {
		params.Tools = tools
	}
	completion, err := c.client.Chat.Completions.New(ctx, params)
	if err != nil {
		return Response{}, err
	}
	if len(completion.Choices) == 0 {
		return Response{}, fmt.Errorf("deepseek chat completion returned no choices")
	}
	choice := completion.Choices[0]
	toolCalls, err := parseDeepSeekToolCalls(choice.Message.ToolCalls)
	if err != nil {
		return Response{}, err
	}
	reasoningContent, err := extractReasoningContent(choice.Message.RawJSON())
	if err != nil {
		return Response{}, err
	}
	return Response{
		Text:             choice.Message.Content,
		ReasoningContent: reasoningContent,
		Model:            completion.Model,
		Usage: Usage{
			InputTokens:  int(completion.Usage.PromptTokens),
			OutputTokens: int(completion.Usage.CompletionTokens),
			TotalTokens:  int(completion.Usage.TotalTokens),
		},
		RawPayload:   []byte(completion.RawJSON()),
		FinishReason: string(choice.FinishReason),
		ToolCalls:    toolCalls,
	}, nil
}

func assistantToolCallMessage(message Message) openai.ChatCompletionMessageParamUnion {
	raw := map[string]any{
		"role": "assistant",
	}
	if strings.TrimSpace(message.Content) != "" || len(message.ToolCalls) == 0 {
		raw["content"] = message.Content
	}
	if strings.TrimSpace(message.ReasoningContent) != "" {
		raw["reasoning_content"] = message.ReasoningContent
	}
	toolCalls := make([]map[string]any, 0, len(message.ToolCalls))
	for _, call := range message.ToolCalls {
		toolCalls = append(toolCalls, map[string]any{
			"id":   call.ID,
			"type": "function",
			"function": map[string]any{
				"name":      call.Name,
				"arguments": call.RawArguments,
			},
		})
	}
	if len(toolCalls) > 0 {
		raw["tool_calls"] = toolCalls
	}
	encoded, _ := json.Marshal(raw)
	assistant := param.Override[openai.ChatCompletionAssistantMessageParam](json.RawMessage(encoded))
	return openai.ChatCompletionMessageParamUnion{OfAssistant: &assistant}
}

func parseDeepSeekToolCalls(calls []openai.ChatCompletionMessageToolCall) ([]ToolCall, error) {
	parsed := make([]ToolCall, 0, len(calls))
	for _, call := range calls {
		args := make(map[string]string)
		if strings.TrimSpace(call.Function.Arguments) != "" {
			var raw map[string]any
			if err := json.Unmarshal([]byte(call.Function.Arguments), &raw); err != nil {
				return nil, fmt.Errorf("parse tool call arguments for %s: %w", call.Function.Name, err)
			}
			for key, value := range raw {
				switch typed := value.(type) {
				case string:
					args[key] = typed
				case bool:
					args[key] = fmt.Sprintf("%t", typed)
				case float64:
					args[key] = fmt.Sprintf("%.0f", typed)
				default:
					encoded, err := json.Marshal(typed)
					if err != nil {
						return nil, err
					}
					args[key] = string(encoded)
				}
			}
		}
		parsed = append(parsed, ToolCall{
			ID:           call.ID,
			Name:         call.Function.Name,
			Arguments:    args,
			RawArguments: call.Function.Arguments,
		})
	}
	return parsed, nil
}

func extractReasoningContent(rawJSON string) (string, error) {
	if strings.TrimSpace(rawJSON) == "" {
		return "", nil
	}
	var body struct {
		ReasoningContent string `json:"reasoning_content"`
	}
	if err := json.Unmarshal([]byte(rawJSON), &body); err != nil {
		return "", err
	}
	return body.ReasoningContent, nil
}
