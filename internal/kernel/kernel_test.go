package kernel

import (
	"context"
	"testing"

	"prismagent/internal/contextprovider"
	"prismagent/internal/core"
	"prismagent/internal/filestore"
)

func TestStartRunCreatesInitialState(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)
	kernel := New(store, fixedIDs{})

	result, err := kernel.StartRun(ctx, StartRunRequest{
		WorkspaceRoot: root,
		Goal:          "build an initial run",
	})
	if err != nil {
		t.Fatalf("start run: %v", err)
	}

	if result.Workspace.ID != "workspace-1" {
		t.Fatalf("unexpected workspace id: %s", result.Workspace.ID)
	}
	if result.Run.ID != "run-1" {
		t.Fatalf("unexpected run id: %s", result.Run.ID)
	}
	if result.RootTask.Status != core.TaskReady {
		t.Fatalf("unexpected task status: %s", result.RootTask.Status)
	}
	if result.InitialObject.Kind != core.ContextPlan {
		t.Fatalf("unexpected context object kind: %s", result.InitialObject.Kind)
	}

	tasks, err := store.ListTasks(ctx, result.Run.ID)
	if err != nil {
		t.Fatalf("list tasks: %v", err)
	}
	if len(tasks) != 1 || tasks[0].ID != result.RootTask.ID {
		t.Fatalf("unexpected tasks: %#v", tasks)
	}

	events, err := store.ListEvents(ctx, result.Run.ID)
	if err != nil {
		t.Fatalf("list events: %v", err)
	}
	if len(events) != 3 {
		t.Fatalf("expected 3 events, got %d", len(events))
	}

	snapshot, err := store.GetSnapshot(ctx, result.Snapshot.ID)
	if err != nil {
		t.Fatalf("get snapshot: %v", err)
	}
	if len(snapshot.State.Tasks) != 1 || len(snapshot.State.ContextObjects) != 1 {
		t.Fatalf("unexpected snapshot state: %#v", snapshot.State)
	}
}

func TestNewRunWithMessageCreatesAgentConversationAndArtifacts(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)
	kernel := NewWithServices(store, fixedIDs{}, nil, fixedContextProvider{})

	result, err := kernel.NewRun(ctx, NewRunRequest{
		WorkspaceRoot: root,
		Message:       "summarize this workspace",
	})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	if result.Agent.ID != "agent-0" {
		t.Fatalf("unexpected agent id: %s", result.Agent.ID)
	}
	if result.Turn == nil {
		t.Fatal("expected initial message to produce a turn")
	}
	if result.Turn.Answer == "" {
		t.Fatal("expected answer")
	}

	turns, err := store.ListConversationTurns(ctx, result.Run.ID)
	if err != nil {
		t.Fatalf("list conversation: %v", err)
	}
	if len(turns) != 2 {
		t.Fatalf("expected user and agent turns, got %d", len(turns))
	}

	contextBody, err := store.ReadRunArtifact(ctx, result.Run.ID, "context.md")
	if err != nil {
		t.Fatalf("read context artifact: %v", err)
	}
	if contextBody != "fixed local context" {
		t.Fatalf("unexpected context: %q", contextBody)
	}
	answer, err := store.ReadRunArtifact(ctx, result.Run.ID, "answer.md")
	if err != nil {
		t.Fatalf("read answer artifact: %v", err)
	}
	if answer != result.Turn.Answer {
		t.Fatalf("answer artifact mismatch")
	}
}

func TestListAndResumeRuns(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)
	kernel := NewWithServices(store, fixedIDs{}, nil, fixedContextProvider{})

	result, err := kernel.NewRun(ctx, NewRunRequest{
		WorkspaceRoot: root,
		Message:       "hello",
	})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	runs, err := kernel.ListRuns(ctx, root)
	if err != nil {
		t.Fatalf("list runs: %v", err)
	}
	if len(runs) != 1 || runs[0].ID != result.Run.ID {
		t.Fatalf("unexpected runs: %#v", runs)
	}
	resumed, err := kernel.ResumeRun(ctx, root, result.Run.ID)
	if err != nil {
		t.Fatalf("resume run: %v", err)
	}
	if len(resumed.Agents) != 1 {
		t.Fatalf("expected one agent, got %d", len(resumed.Agents))
	}
	if len(resumed.Conversation) != 2 {
		t.Fatalf("expected two conversation turns, got %d", len(resumed.Conversation))
	}
}

func TestStartRunRejectsEmptyGoal(t *testing.T) {
	ctx := context.Background()
	store := filestore.New(t.TempDir())
	kernel := New(store, fixedIDs{})

	if _, err := kernel.StartRun(ctx, StartRunRequest{Goal: "   "}); err == nil {
		t.Fatal("expected empty goal to be rejected")
	}
}

type fixedIDs struct{}

func (fixedIDs) WorkspaceID() core.WorkspaceID         { return "workspace-1" }
func (fixedIDs) RunID() core.RunID                     { return "run-1" }
func (fixedIDs) TaskID() core.TaskID                   { return "task-1" }
func (fixedIDs) ContextObjectID() core.ContextObjectID { return "ctx-1" }
func (fixedIDs) SnapshotID() core.SnapshotID           { return "snapshot-1" }

type fixedContextProvider struct{}

func (fixedContextProvider) Collect(context.Context, string) (contextprovider.Bundle, error) {
	return contextprovider.Bundle{
		Text:  "fixed local context",
		Files: []string{"README.md"},
	}, nil
}
