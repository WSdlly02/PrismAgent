package memory

import (
	"context"
	"testing"
	"time"

	"prismagent/internal/core"
)

func TestRuntimeStoresTasksAndContextObjects(t *testing.T) {
	ctx := context.Background()
	runtime := NewRuntime()
	runID := core.RunID("run-1")
	task := core.NewTask("task-1", runID, "test task")

	if err := runtime.CreateTask(ctx, task); err != nil {
		t.Fatalf("create task: %v", err)
	}
	object := core.NewContextObject("ctx-1", core.ContextFact, core.ContextScope{
		Type:  core.ScopeRun,
		RunID: runID,
	}, "a stable fact")
	if err := runtime.PutContextObject(ctx, object); err != nil {
		t.Fatalf("put context object: %v", err)
	}

	tasks, err := runtime.ListTasks(ctx, runID)
	if err != nil {
		t.Fatalf("list tasks: %v", err)
	}
	if len(tasks) != 1 {
		t.Fatalf("expected 1 task, got %d", len(tasks))
	}

	objects, err := runtime.ListContextObjects(ctx, core.ContextScope{Type: core.ScopeRun, RunID: runID})
	if err != nil {
		t.Fatalf("list context objects: %v", err)
	}
	if len(objects) != 1 {
		t.Fatalf("expected 1 context object, got %d", len(objects))
	}
}

func TestRuntimeRestoresSnapshot(t *testing.T) {
	ctx := context.Background()
	runtime := NewRuntime()
	runID := core.RunID("run-1")
	task := core.NewTask("task-1", runID, "test task")

	if err := runtime.CreateTask(ctx, task); err != nil {
		t.Fatalf("create task: %v", err)
	}
	if err := runtime.CaptureSnapshot(ctx, core.NewSnapshot("snapshot-1", runID, "before mutation")); err != nil {
		t.Fatalf("capture snapshot: %v", err)
	}

	if err := task.Transition(core.TaskDone, time.Now()); err != nil {
		t.Fatalf("transition task: %v", err)
	}
	if err := runtime.UpdateTask(ctx, task); err != nil {
		t.Fatalf("update task: %v", err)
	}

	if _, err := runtime.RestoreSnapshot(ctx, "snapshot-1"); err != nil {
		t.Fatalf("restore snapshot: %v", err)
	}
	restored, err := runtime.GetTask(ctx, "task-1")
	if err != nil {
		t.Fatalf("get restored task: %v", err)
	}
	if restored.Status != core.TaskReady {
		t.Fatalf("expected restored status %s, got %s", core.TaskReady, restored.Status)
	}
}
