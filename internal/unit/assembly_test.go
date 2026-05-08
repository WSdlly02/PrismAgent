package unit

import (
	"testing"
	"time"

	"prismagent/internal/atom"
	"prismagent/internal/core"
)

func putAtom(t *testing.T, store *atom.Store, data []byte) string {
	t.Helper()
	hash, err := store.Put(data)
	if err != nil {
		t.Fatalf("atom Put failed: %v", err)
	}
	return hash
}

func TestAssembleMessages(t *testing.T) {
	tmp := t.TempDir()
	atomStore := atom.NewStore(tmp)

	sysHash := putAtom(t, atomStore, []byte("You are a helpful assistant."))
	userHash := putAtom(t, atomStore, []byte("Hello, world!"))
	assistantHash := putAtom(t, atomStore, []byte(`{
  "choices": [{
    "message": {
      "content": "Hi there!",
      "reasoning_content": "The user said hello."
    }
  }],
  "model": "deepseek-chat",
  "usage": {"prompt_tokens": 10, "completion_tokens": 5}
}`))

	now := time.Now()
	chain := []core.Unit{
		{UUID: "u1", AtomHash: sysHash, Kind: core.UnitMessage, Role: core.RoleSystem, CreatedAt: now},
		{UUID: "u2", AtomHash: userHash, Kind: core.UnitMessage, Role: core.RoleUser, CreatedAt: now},
		{UUID: "u3", AtomHash: assistantHash, Kind: core.UnitLLMResp, Role: core.RoleAssistant, CreatedAt: now},
	}

	msgs, err := AssembleMessages(chain, atomStore)
	if err != nil {
		t.Fatalf("AssembleMessages failed: %v", err)
	}

	if len(msgs) != 3 {
		t.Fatalf("expected 3 messages, got %d", len(msgs))
	}

	// System message
	if msgs[0].Role != "system" {
		t.Fatalf("msgs[0] role: got %s, want system", msgs[0].Role)
	}
	if msgs[0].Content != "You are a helpful assistant." {
		t.Fatalf("msgs[0] content: got %q", msgs[0].Content)
	}

	// User message
	if msgs[1].Role != "user" {
		t.Fatalf("msgs[1] role: got %s, want user", msgs[1].Role)
	}
	if msgs[1].Content != "Hello, world!" {
		t.Fatalf("msgs[1] content: got %q", msgs[1].Content)
	}

	// Assistant LLM response
	if msgs[2].Role != "assistant" {
		t.Fatalf("msgs[2] role: got %s, want assistant", msgs[2].Role)
	}
	if msgs[2].Content != "Hi there!" {
		t.Fatalf("msgs[2] content: got %q", msgs[2].Content)
	}
	if msgs[2].ReasoningContent != "The user said hello." {
		t.Fatalf("msgs[2] reasoning: got %q", msgs[2].ReasoningContent)
	}
}

func TestAssembleToolCallSkipped(t *testing.T) {
	tmp := t.TempDir()
	atomStore := atom.NewStore(tmp)

	userHash := putAtom(t, atomStore, []byte("read a file"))
	toolCallHash := putAtom(t, atomStore, []byte("tool call audit data"))

	now := time.Now()
	chain := []core.Unit{
		{UUID: "u1", AtomHash: userHash, Kind: core.UnitMessage, Role: core.RoleUser, CreatedAt: now},
		{UUID: "u2", AtomHash: toolCallHash, Kind: core.UnitToolCall, Role: core.RoleAssistant, CreatedAt: now},
	}

	msgs, err := AssembleMessages(chain, atomStore)
	if err != nil {
		t.Fatalf("AssembleMessages failed: %v", err)
	}

	if len(msgs) != 1 {
		t.Fatalf("expected 1 message (tool call should be skipped), got %d", len(msgs))
	}
	if msgs[0].Role != "user" {
		t.Fatalf("expected user message, got %s", msgs[0].Role)
	}
}

func TestAssembleToolResult(t *testing.T) {
	tmp := t.TempDir()
	atomStore := atom.NewStore(tmp)

	resultHash := putAtom(t, atomStore, []byte("file contents here"))

	now := time.Now()
	chain := []core.Unit{
		{
			UUID:     "u1",
			AtomHash: resultHash,
			Kind:     core.UnitToolResult,
			Role:     core.RoleTool,
			Metadata: map[string]string{"tool_call_id": "call_abc123"},
			CreatedAt: now,
		},
	}

	msgs, err := AssembleMessages(chain, atomStore)
	if err != nil {
		t.Fatalf("AssembleMessages failed: %v", err)
	}

	if len(msgs) != 1 {
		t.Fatalf("expected 1 message, got %d", len(msgs))
	}
	if msgs[0].Role != "tool" {
		t.Fatalf("role: got %s, want tool", msgs[0].Role)
	}
	if msgs[0].ToolCallID != "call_abc123" {
		t.Fatalf("ToolCallID: got %s, want call_abc123", msgs[0].ToolCallID)
	}
	if msgs[0].Content != "file contents here" {
		t.Fatalf("Content: got %q", msgs[0].Content)
	}
}

func TestAssembleLLMResponseWithToolCalls(t *testing.T) {
	tmp := t.TempDir()
	atomStore := atom.NewStore(tmp)

	llmHash := putAtom(t, atomStore, []byte(`{
  "choices": [{
    "message": {
      "content": "",
      "reasoning_content": "",
      "tool_calls": [
        {
          "id": "call_001",
          "function": {
            "name": "read_file",
            "arguments": "{\"path\": \"/tmp/test.txt\"}"
          }
        }
      ]
    }
  }],
  "model": "deepseek-chat"
}`))

	now := time.Now()
	chain := []core.Unit{
		{UUID: "u1", AtomHash: llmHash, Kind: core.UnitLLMResp, Role: core.RoleAssistant, CreatedAt: now},
	}

	msgs, err := AssembleMessages(chain, atomStore)
	if err != nil {
		t.Fatalf("AssembleMessages failed: %v", err)
	}

	if len(msgs) != 1 {
		t.Fatalf("expected 1 message, got %d", len(msgs))
	}
	if len(msgs[0].ToolCalls) != 1 {
		t.Fatalf("expected 1 tool call, got %d", len(msgs[0].ToolCalls))
	}
	if msgs[0].ToolCalls[0].ID != "call_001" {
		t.Fatalf("tool call ID: got %s, want call_001", msgs[0].ToolCalls[0].ID)
	}
	if msgs[0].ToolCalls[0].Name != "read_file" {
		t.Fatalf("tool call Name: got %s, want read_file", msgs[0].ToolCalls[0].Name)
	}
	if msgs[0].ToolCalls[0].RawArguments != `{"path": "/tmp/test.txt"}` {
		t.Fatalf("tool call RawArguments: got %s", msgs[0].ToolCalls[0].RawArguments)
	}
}
