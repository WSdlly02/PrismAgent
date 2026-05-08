package kernel

import (
	"context"
	"fmt"
	"testing"

	"prismagent/internal/contextprovider"
	"prismagent/internal/core"
	"prismagent/internal/filestore"
	"prismagent/internal/model"
	"prismagent/internal/unit"
)

func TestNewRunWithMessageCreatesAgentConversationAndArtifacts(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)
	kernel := NewWithServices(store, fixedIDs{}, nil, fixedContextProvider{}, root)

	result, err := kernel.NewRun(ctx, NewRunRequest{
		WorkspaceRoot: root,
		Message:       "summarize this workspace",
	})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	if result.Agent.ID != "0" {
		t.Fatalf("unexpected agent id: %s", result.Agent.ID)
	}
	if result.Turn == nil {
		t.Fatal("expected initial message to produce a turn")
	}
	if result.Turn.Answer == "" {
		t.Fatal("expected answer")
	}

	// Verify agent chain has system + user + llm_response units
	chain, err := unit.LoadChain(root, string(result.Run.ID), "0")
	if err != nil {
		t.Fatalf("load chain: %v", err)
	}
	if len(chain.Chain) != 3 {
		t.Fatalf("expected 3 units in chain (system+user+llm), got %d", len(chain.Chain))
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
	current, err := kernel.CurrentRun(ctx, root)
	if err != nil {
		t.Fatalf("current run: %v", err)
	}
	if current != result.Run.ID {
		t.Fatalf("expected current run %s, got %s", result.Run.ID, current)
	}
}

func TestListAndResumeRuns(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)
	kernel := NewWithServices(store, fixedIDs{}, nil, fixedContextProvider{}, root)

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
	if resumed.Answer == "" {
		t.Fatal("expected answer in resumed run")
	}
}

func TestResumeRunSetsCurrentRun(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)
	kernel := NewWithServices(store, &incrementingIDs{}, nil, fixedContextProvider{}, root)

	first, err := kernel.NewRun(ctx, NewRunRequest{WorkspaceRoot: root, Message: "first"})
	if err != nil {
		t.Fatalf("first run: %v", err)
	}
	second, err := kernel.NewRun(ctx, NewRunRequest{WorkspaceRoot: root, Message: "second"})
	if err != nil {
		t.Fatalf("second run: %v", err)
	}
	if _, err := kernel.ResumeRun(ctx, root, first.Run.ID); err != nil {
		t.Fatalf("resume first run: %v", err)
	}
	current, err := kernel.CurrentRun(ctx, root)
	if err != nil {
		t.Fatalf("current run: %v", err)
	}
	if current != first.Run.ID {
		t.Fatalf("expected current run %s, got %s; second was %s", first.Run.ID, current, second.Run.ID)
	}
}

func TestRunMessageIncludesConversationHistoryInModelPrompt(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := filestore.New(root)
	recorder := &recordingModel{}
	kernel := NewWithServices(store, &incrementingIDs{}, recorder, fixedContextProvider{}, root)

	result, err := kernel.NewRun(ctx, NewRunRequest{WorkspaceRoot: root, Message: "first question"})
	if err != nil {
		t.Fatalf("new run: %v", err)
	}
	if _, err := kernel.RunMessage(ctx, RunTurnRequest{
		WorkspaceRoot: root,
		RunID:         result.Run.ID,
		Message:       "second question",
	}); err != nil {
		t.Fatalf("run message: %v", err)
	}
	if len(recorder.requests) != 2 {
		t.Fatalf("expected 2 model requests, got %d", len(recorder.requests))
	}
	second := recorder.requests[1]
	if !containsMessage(second.Messages, "user", "first question") {
		t.Fatalf("second request missing first user turn: %#v", second.Messages)
	}
	if !containsMessage(second.Messages, "assistant", "recorded response 1") {
		t.Fatalf("second request missing first assistant turn: %#v", second.Messages)
	}
	if !containsMessage(second.Messages, "user", "second question") {
		t.Fatalf("second request missing second user turn: %#v", second.Messages)
	}
}

type fixedIDs struct{}

func (fixedIDs) WorkspaceID() core.WorkspaceID { return "workspace-1" }
func (fixedIDs) RunID() core.RunID             { return "run-1" }

type fixedContextProvider struct{}

func (fixedContextProvider) Collect(context.Context, string) (contextprovider.Bundle, error) {
	return contextprovider.Bundle{
		Text:  "fixed local context",
		Files: []string{"README.md"},
	}, nil
}

type incrementingIDs struct {
	next int
}

func (g *incrementingIDs) WorkspaceID() core.WorkspaceID { return "workspace-1" }
func (g *incrementingIDs) RunID() core.RunID {
	g.next++
	return core.RunID(fmt.Sprintf("run-%d", g.next))
}

type recordingModel struct {
	requests []model.Request
}

func (m *recordingModel) Complete(_ context.Context, req model.Request) (model.Response, error) {
	m.requests = append(m.requests, req)
	return model.Response{
		Text:  fmt.Sprintf("recorded response %d", len(m.requests)),
		Model: "recording",
	}, nil
}

func containsMessage(messages []model.Message, role string, content string) bool {
	for _, message := range messages {
		if message.Role == role && message.Content == content {
			return true
		}
	}
	return false
}
