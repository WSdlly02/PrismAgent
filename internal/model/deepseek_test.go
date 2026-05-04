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
