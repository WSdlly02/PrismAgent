package model

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestDeepSeekConfigFromEnvRequiresAllValues(t *testing.T) {
	t.Setenv(DeepSeekAPIKeyEnv, "key")
	t.Setenv(DeepSeekAPIBaseURLEnv, "https://api.deepseek.com")
	t.Setenv(DeepSeekAPIModelEnv, "deepseek-chat")

	cfg, ok := DeepSeekConfigFromEnv()
	if !ok {
		t.Fatal("expected complete DeepSeek config")
	}
	if cfg.APIKey != "key" || cfg.BaseURL != "https://api.deepseek.com" || cfg.Model != "deepseek-chat" {
		t.Fatalf("unexpected config: %#v", cfg)
	}
}

func TestDeepSeekConfigFromEnvRejectsPartialValues(t *testing.T) {
	t.Setenv(DeepSeekAPIKeyEnv, "key")
	t.Setenv(DeepSeekAPIBaseURLEnv, "")
	t.Setenv(DeepSeekAPIModelEnv, "deepseek-chat")

	if _, ok := DeepSeekConfigFromEnv(); ok {
		t.Fatal("expected partial config to be rejected")
	}
}

func TestDeepSeekClientUsesChatCompletionsCompatibleEndpoint(t *testing.T) {
	var seenPath string
	var seenModel string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seenPath = r.URL.Path
		var body struct {
			Model string `json:"model"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		seenModel = body.Model
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"id": "chatcmpl-test",
			"object": "chat.completion",
			"created": 1,
			"model": "deepseek-chat",
			"choices": [{
				"index": 0,
				"message": {"role": "assistant", "content": "hello from deepseek"},
				"finish_reason": "stop",
				"logprobs": null
			}],
			"usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5}
		}`))
	}))
	defer server.Close()

	client, err := NewDeepSeekClient(DeepSeekConfig{
		APIKey:  "test-key",
		BaseURL: server.URL,
		Model:   "deepseek-chat",
	})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	resp, err := client.Complete(context.Background(), Request{
		Messages: []Message{{Role: "user", Content: "hello"}},
	})
	if err != nil {
		t.Fatalf("complete: %v", err)
	}
	if seenPath != "/chat/completions" {
		t.Fatalf("unexpected path: %s", seenPath)
	}
	if seenModel != "deepseek-chat" {
		t.Fatalf("unexpected model: %s", seenModel)
	}
	if resp.Text != "hello from deepseek" {
		t.Fatalf("unexpected response: %q", resp.Text)
	}
	if resp.Usage.TotalTokens != 5 {
		t.Fatalf("unexpected usage: %#v", resp.Usage)
	}
}

func TestDeepSeekClientSendsToolsAndParsesToolCalls(t *testing.T) {
	var sawTools bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Tools []any `json:"tools"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		sawTools = len(body.Tools) == 1
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"id": "chatcmpl-test",
			"object": "chat.completion",
			"created": 1,
			"model": "deepseek-chat",
			"choices": [{
				"index": 0,
				"message": {
					"role": "assistant",
					"content": "",
					"tool_calls": [{
						"id": "call_1",
						"type": "function",
						"function": {
							"name": "read_file",
							"arguments": "{\"path\":\"README.md\",\"limit\":12000}"
						}
					}]
				},
				"finish_reason": "tool_calls",
				"logprobs": null
			}],
			"usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5}
		}`))
	}))
	defer server.Close()

	client, err := NewDeepSeekClient(DeepSeekConfig{
		APIKey:  "test-key",
		BaseURL: server.URL,
		Model:   "deepseek-chat",
	})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	resp, err := client.Complete(context.Background(), Request{
		Messages: []Message{{Role: "user", Content: "read readme"}},
		Tools: []ToolDefinition{{
			Name:        "read_file",
			Description: "Read a file",
			Parameters:  map[string]any{"type": "object"},
		}},
	})
	if err != nil {
		t.Fatalf("complete: %v", err)
	}
	if !sawTools {
		t.Fatal("expected tools in request")
	}
	if len(resp.ToolCalls) != 1 {
		t.Fatalf("expected 1 tool call, got %d", len(resp.ToolCalls))
	}
	call := resp.ToolCalls[0]
	if call.ID != "call_1" || call.Name != "read_file" || call.Arguments["path"] != "README.md" {
		t.Fatalf("unexpected tool call: %#v", call)
	}
}

func TestDeepSeekClientPreservesReasoningContent(t *testing.T) {
	requestCount := 0
	var secondReasoning string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requestCount++
		var body struct {
			Messages []map[string]any `json:"messages"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		if requestCount == 2 {
			for _, message := range body.Messages {
				if message["role"] == "assistant" {
					if value, ok := message["reasoning_content"].(string); ok {
						secondReasoning = value
					}
				}
			}
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"id": "chatcmpl-test",
			"object": "chat.completion",
			"created": 1,
			"model": "deepseek-reasoner",
			"choices": [{
				"index": 0,
				"message": {
					"role": "assistant",
					"content": "",
					"reasoning_content": "internal reasoning",
					"tool_calls": [{
						"id": "call_1",
						"type": "function",
						"function": {
							"name": "read_file",
							"arguments": "{\"path\":\"README.md\"}"
						}
					}]
				},
				"finish_reason": "tool_calls",
				"logprobs": null
			}],
			"usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5}
		}`))
	}))
	defer server.Close()

	client, err := NewDeepSeekClient(DeepSeekConfig{
		APIKey:  "test-key",
		BaseURL: server.URL,
		Model:   "deepseek-reasoner",
	})
	if err != nil {
		t.Fatalf("new client: %v", err)
	}
	first, err := client.Complete(context.Background(), Request{
		Messages: []Message{{Role: "user", Content: "read"}},
	})
	if err != nil {
		t.Fatalf("first complete: %v", err)
	}
	if first.ReasoningContent != "internal reasoning" {
		t.Fatalf("unexpected reasoning content: %q", first.ReasoningContent)
	}
	if _, err := client.Complete(context.Background(), Request{
		Messages: []Message{
			{Role: "user", Content: "read"},
			{
				Role:             "assistant",
				ReasoningContent: first.ReasoningContent,
				ToolCalls:        first.ToolCalls,
			},
			{Role: "tool", ToolCallID: "call_1", Content: "tool result"},
		},
	}); err != nil {
		t.Fatalf("second complete: %v", err)
	}
	if secondReasoning != "internal reasoning" {
		t.Fatalf("expected reasoning content to be sent back, got %q", secondReasoning)
	}
}
