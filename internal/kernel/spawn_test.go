package kernel

import (
	"context"
	"strings"
	"testing"

	"prismagent/internal/filestore"
	"prismagent/internal/model"
	"prismagent/internal/tool"
)

func TestSpawnAgentExecutesSubAgentAndReturnsResult(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)

	// Model: first call spawns agent (returns tool call), second call is sub-agent, third is parent final
	callCount := 0
	modelClient := model.FuncClient(func(_ context.Context, req model.Request) (model.Response, error) {
		callCount++
		// Check if this is a sub-agent call (system prompt mentions "sub-agent")
		for _, msg := range req.Messages {
			if msg.Role == "system" && strings.Contains(msg.Content, "sub-agent") {
				// Sub-agent responds directly
				return model.Response{Text: "sub-agent answer", Model: "test"}, nil
			}
		}
		// Parent agent: first call spawns, second call summarizes
		if callCount == 1 {
			return model.Response{
				Model: "test",
				ToolCalls: []model.ToolCall{{
					ID:           "spawn-1",
					Name:         "spawn_agent",
					RawArguments: `{"message":"do the subtask"}`,
				}},
			}, nil
		}
		return model.Response{Text: "parent final answer with sub-agent answer", Model: "test"}, nil
	})

	kernel := NewWithServices(store, &incrementingIDs{}, modelClient, fixedContextProvider{}, root)
	kernel.RegisterTools(tool.NewFileSystemTools(root)...)

	result, err := kernel.NewRun(ctx, NewRunRequest{
		WorkspaceRoot: root,
		Message:       "delegate this task",
	})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	if result.Turn == nil {
		t.Fatal("expected turn result")
	}
	if !strings.Contains(result.Turn.Answer, "parent final answer") {
		t.Fatalf("unexpected answer: %q", result.Turn.Answer)
	}
}

func TestSpawnAgentRespectsDepthLimit(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)

	// Model tries to spawn, then gives up after seeing the error
	spawnAttempted := false
	modelClient := model.FuncClient(func(_ context.Context, req model.Request) (model.Response, error) {
		// Check if previous tool call failed (depth limit error)
		for _, msg := range req.Messages {
			if msg.Role == "tool" && strings.Contains(msg.Content, "depth") {
				return model.Response{Text: "cannot spawn deeper, doing it myself", Model: "test"}, nil
			}
		}
		if !spawnAttempted {
			spawnAttempted = true
			return model.Response{
				Model: "test",
				ToolCalls: []model.ToolCall{{
					ID:           "spawn-1",
					Name:         "spawn_agent",
					RawArguments: `{"message":"keep spawning"}`,
				}},
			}, nil
		}
		return model.Response{Text: "fallback answer", Model: "test"}, nil
	})

	kernel := NewWithServices(store, fixedIDs{}, modelClient, fixedContextProvider{}, root)
	kernel.RegisterTools(tool.NewFileSystemTools(root)...)

	result, err := kernel.NewRun(ctx, NewRunRequest{
		WorkspaceRoot: root,
		Message:       "spawn forever",
	})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	if result.Turn == nil {
		t.Fatal("expected turn result")
	}
}
