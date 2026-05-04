package tool

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestFileSystemToolsReadAndListWorkspaceFiles(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("hello"), 0o644); err != nil {
		t.Fatalf("write file: %v", err)
	}
	registry := NewRegistry(NewFileSystemTools(root)...)

	list, err := registry.Call(ctx, Request{
		Name: ToolListFiles,
		Args: map[string]string{"path": ".", "recursive": "false"},
	})
	if err != nil {
		t.Fatalf("list files: %v", err)
	}
	if !strings.Contains(list.RawOutput, "README.md") {
		t.Fatalf("list output missing README: %s", list.RawOutput)
	}

	read, err := registry.Call(ctx, Request{
		Name: ToolReadFile,
		Args: map[string]string{"path": "README.md"},
	})
	if err != nil {
		t.Fatalf("read file: %v", err)
	}
	if read.RawOutput != "hello" {
		t.Fatalf("unexpected read output: %q", read.RawOutput)
	}
}

func TestReplaceInFileRequiresUniqueMatch(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	path := filepath.Join(root, "file.txt")
	if err := os.WriteFile(path, []byte("x x"), 0o644); err != nil {
		t.Fatalf("write file: %v", err)
	}
	registry := NewRegistry(NewFileSystemTools(root)...)

	if _, err := registry.Call(ctx, Request{
		Name: ToolReplaceInFile,
		Args: map[string]string{"path": "file.txt", "old": "x", "new": "y"},
	}); err == nil {
		t.Fatal("expected non-unique replacement to fail")
	}
	if _, err := registry.Call(ctx, Request{
		Name: ToolReplaceInFile,
		Args: map[string]string{"path": "file.txt", "old": "x x", "new": "y"},
	}); err != nil {
		t.Fatalf("replace unique text: %v", err)
	}
	updated, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read updated file: %v", err)
	}
	if string(updated) != "y" {
		t.Fatalf("unexpected updated content: %q", string(updated))
	}
}

func TestFileSystemToolsBlockWorkspaceEscape(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	outside := filepath.Join(t.TempDir(), "outside.txt")
	if err := os.WriteFile(outside, []byte("secret"), 0o644); err != nil {
		t.Fatalf("write outside file: %v", err)
	}
	registry := NewRegistry(NewFileSystemTools(root)...)

	if _, err := registry.Call(ctx, Request{
		Name: ToolReadFile,
		Args: map[string]string{"path": "../outside.txt"},
	}); err == nil {
		t.Fatal("expected workspace escape to fail")
	}
}
