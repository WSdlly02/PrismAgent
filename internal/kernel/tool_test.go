package kernel

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"prismagent/internal/filestore"
	"prismagent/internal/tool"
)

func TestKernelCallToolEmitsEvents(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("hello"), 0o644); err != nil {
		t.Fatalf("write file: %v", err)
	}
	store := filestore.New(root)
	kernel := NewWithServices(store, fixedIDs{}, nil, fixedContextProvider{})
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
