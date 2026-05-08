package kernel

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"prismagent/internal/filestore"
	"prismagent/internal/model"
	"prismagent/internal/tool"
)

func TestKernelCallToolEmitsEvents(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("hello"), 0o644); err != nil {
		t.Fatalf("write file: %v", err)
	}
	store := filestore.New(root)
	kernel := NewWithServices(store, fixedIDs{}, nil, fixedContextProvider{}, root)
	kernel.RegisterTools(tool.NewFileSystemTools(root)...)

	run, err := kernel.NewRun(ctx, NewRunRequest{WorkspaceRoot: root})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	result, err := kernel.CallTool(ctx, ToolCallRequest{
		RunID: run.Run.ID,
		Name:  tool.ToolReadFile,
		Args:  map[string]string{"path": "README.md"},
	})
	if err != nil {
		t.Fatalf("call tool: %v", err)
	}
	if result.RawOutput != "hello" {
		t.Fatalf("unexpected tool output: %q", result.RawOutput)
	}
	events, err := store.ListEvents(ctx, run.Run.ID)
	if err != nil {
		t.Fatalf("list events: %v", err)
	}
	if len(events) < 4 {
		t.Fatalf("expected tool events, got %#v", events)
	}
}

func TestRunMessageExecutesModelToolCallAndReturnsFinalAnswer(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("project readme"), 0o644); err != nil {
		t.Fatalf("write file: %v", err)
	}
	store := filestore.New(root)
	modelClient := &scriptedToolModel{}
	kernel := NewWithServices(store, &incrementingIDs{}, modelClient, fixedContextProvider{}, root)
	kernel.RegisterTools(tool.NewFileSystemTools(root)...)

	run, err := kernel.NewRun(ctx, NewRunRequest{WorkspaceRoot: root})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	result, err := kernel.RunMessage(ctx, RunTurnRequest{
		WorkspaceRoot: root,
		RunID:         run.Run.ID,
		Message:       "read README",
	})
	if err != nil {
		t.Fatalf("run message: %v", err)
	}
	if result.Answer != "final answer using tool result" {
		t.Fatalf("unexpected answer: %q", result.Answer)
	}
	if len(modelClient.requests) != 2 {
		t.Fatalf("expected 2 model requests, got %d", len(modelClient.requests))
	}
	second := modelClient.requests[1]
	if !containsMessage(second.Messages, "tool", "project readme") {
		t.Fatalf("second request missing tool result: %#v", second.Messages)
	}
	if !containsReasoning(second.Messages, "reasoning before tool") {
		t.Fatalf("second request missing reasoning content: %#v", second.Messages)
	}
}

type scriptedToolModel struct {
	requests []model.Request
}

func (m *scriptedToolModel) Complete(_ context.Context, req model.Request) (model.Response, error) {
	m.requests = append(m.requests, req)
	if len(m.requests) == 1 {
		return model.Response{
			Model:            "scripted",
			ReasoningContent: "reasoning before tool",
			ToolCalls: []model.ToolCall{{
				ID:           "call-1",
				Name:         tool.ToolReadFile,
				Arguments:    map[string]string{"path": "README.md", "limit": "12000"},
				RawArguments: `{"path":"README.md","limit":"12000"}`,
			}},
		}, nil
	}
	return model.Response{
		Text:  "final answer using tool result",
		Model: "scripted",
	}, nil
}

func containsReasoning(messages []model.Message, reasoning string) bool {
	for _, message := range messages {
		if message.ReasoningContent == reasoning {
			return true
		}
	}
	return false
}
