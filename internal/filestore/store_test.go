package filestore

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"prismagent/internal/core"
)

func TestStoreInitializesWorkspaceAndRun(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := New(root)

	workspace := core.NewWorkspace("workspace-1", root)
	if err := store.InitWorkspace(ctx, workspace); err != nil {
		t.Fatalf("init workspace: %v", err)
	}
	run := core.NewRun("run-1", workspace.ID, "test goal")
	if err := store.CreateRun(ctx, run); err != nil {
		t.Fatalf("create run: %v", err)
	}

	assertFileExists(t, filepath.Join(root, ".prismagent", "workspace.json"))
	assertFileExists(t, filepath.Join(root, ".prismagent", "config.toml"))
	assertFileExists(t, filepath.Join(root, ".prismagent", "runs", "run-1", "run.json"))
	assertFileExists(t, filepath.Join(root, ".prismagent", "runs", "run-1", "snapshots"))
	assertFileExists(t, filepath.Join(root, ".prismagent", "runs", "run-1", "artifacts"))
}

func TestStorePersistsAgentsAndRunArtifacts(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := New(root)
	runID := core.RunID("run-1")

	if err := store.InitWorkspace(ctx, core.NewWorkspace("workspace-1", root)); err != nil {
		t.Fatalf("init workspace: %v", err)
	}
	if err := store.CreateRun(ctx, core.NewRun(runID, "workspace-1", "test goal")); err != nil {
		t.Fatalf("create run: %v", err)
	}

	agent := core.NewRootAgent(runID)
	if err := store.WriteAgents(ctx, runID, []core.Agent{agent}); err != nil {
		t.Fatalf("write agents: %v", err)
	}
	if err := store.WriteRunArtifact(ctx, runID, "answer.md", "world"); err != nil {
		t.Fatalf("write run artifact: %v", err)
	}

	agents, err := store.ListAgents(ctx, runID)
	if err != nil {
		t.Fatalf("list agents: %v", err)
	}
	if len(agents) != 1 || agents[0].ID != agent.ID {
		t.Fatalf("unexpected agents: %#v", agents)
	}
	answer, err := store.ReadRunArtifact(ctx, runID, "answer.md")
	if err != nil {
		t.Fatalf("read run artifact: %v", err)
	}
	if answer != "world" {
		t.Fatalf("unexpected answer: %q", answer)
	}
}

func TestStorePersistsCurrentRunPointer(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := New(root)
	runID := core.RunID("run-1")

	if err := store.InitWorkspace(ctx, core.NewWorkspace("workspace-1", root)); err != nil {
		t.Fatalf("init workspace: %v", err)
	}
	if err := store.CreateRun(ctx, core.NewRun(runID, "workspace-1", "test goal")); err != nil {
		t.Fatalf("create run: %v", err)
	}
	if err := store.SetCurrentRun(ctx, runID); err != nil {
		t.Fatalf("set current run: %v", err)
	}
	current, err := store.GetCurrentRun(ctx)
	if err != nil {
		t.Fatalf("get current run: %v", err)
	}
	if current != runID {
		t.Fatalf("expected current run %s, got %s", runID, current)
	}
}

func TestStorePersistsTasksContextEventsAndSnapshots(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	store := New(root)
	runID := core.RunID("run-1")

	if err := store.InitWorkspace(ctx, core.NewWorkspace("workspace-1", root)); err != nil {
		t.Fatalf("init workspace: %v", err)
	}
	if err := store.CreateRun(ctx, core.NewRun(runID, "workspace-1", "test goal")); err != nil {
		t.Fatalf("create run: %v", err)
	}

	task := core.NewTask("task-1", runID, "test task")
	if err := store.CreateTask(ctx, task); err != nil {
		t.Fatalf("create task: %v", err)
	}
	object := core.NewContextObject("ctx-1", core.ContextFact, core.ContextScope{
		Type:  core.CtxScopeRun,
		RunID: runID,
	}, "a stable fact")
	if err := store.PutContextObject(ctx, object); err != nil {
		t.Fatalf("put context object: %v", err)
	}
	event := core.NewEvent(core.EventRunCreated, runID, task.ID, map[string]string{"goal": task.Goal})
	if err := store.Emit(ctx, event); err != nil {
		t.Fatalf("emit event: %v", err)
	}
	snapshot := core.Snapshot{
		UUID:       "snapshot-1",
		AgentHeads: map[string]string{"agent-0": "unit-last"},
		UnitChains: map[string][]string{"agent-0": {"u1", "u2"}},
		CreatedAt:  time.Now().UTC(),
	}
	if err := store.CreateSnapshot(ctx, snapshot); err != nil {
		t.Fatalf("create snapshot: %v", err)
	}

	reopened := New(root)
	tasks, err := reopened.ListTasks(ctx, runID)
	if err != nil {
		t.Fatalf("list tasks: %v", err)
	}
	if len(tasks) != 1 || tasks[0].ID != task.ID {
		t.Fatalf("unexpected tasks: %#v", tasks)
	}
	objects, err := reopened.ListContextObjects(ctx, core.ContextScope{Type: core.CtxScopeRun, RunID: runID})
	if err != nil {
		t.Fatalf("list context objects: %v", err)
	}
	if len(objects) != 1 || objects[0].ID != object.ID {
		t.Fatalf("unexpected context objects: %#v", objects)
	}
	events, err := reopened.ListEvents(ctx, runID)
	if err != nil {
		t.Fatalf("list events: %v", err)
	}
	if len(events) != 1 || events[0].Type != core.EventRunCreated {
		t.Fatalf("unexpected events: %#v", events)
	}
	got, err := reopened.GetSnapshot(ctx, "snapshot-1")
	if err != nil {
		t.Fatalf("get snapshot: %v", err)
	}
	if got.UUID != snapshot.UUID {
		t.Fatalf("unexpected snapshot: %#v", got)
	}
}

func assertFileExists(t *testing.T, path string) {
	t.Helper()
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("expected %s to exist: %v", path, err)
	}
}
