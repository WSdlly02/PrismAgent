package main

import (
	"context"
	"fmt"
	"os"
	"strings"

	"prismagent/internal/core"
	"prismagent/internal/memory"
)

func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(2)
	}

	switch os.Args[1] {
	case "run":
		if len(os.Args) < 3 {
			fmt.Fprintln(os.Stderr, "missing goal")
			os.Exit(2)
		}
		if err := run(strings.Join(os.Args[2:], " ")); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	default:
		printUsage()
		os.Exit(2)
	}
}

func run(goal string) error {
	ctx := context.Background()
	runtime := memory.NewRuntime()
	runID := core.RunID("run-local")
	task := core.NewTask("task-root", runID, goal)

	if err := runtime.CreateTask(ctx, task); err != nil {
		return err
	}
	if err := runtime.Emit(ctx, core.NewEvent(core.EventTaskCreated, runID, task.ID, map[string]string{
		"goal": goal,
	})); err != nil {
		return err
	}

	object := core.NewContextObject("ctx-goal", core.ContextPlan, core.ContextScope{
		Type:  core.ScopeRun,
		RunID: runID,
	}, goal)
	if err := runtime.PutContextObject(ctx, object); err != nil {
		return err
	}
	if err := runtime.CaptureSnapshot(ctx, core.NewSnapshot("snapshot-initial", runID, "initial run state")); err != nil {
		return err
	}

	fmt.Printf("run_id=%s\n", runID)
	fmt.Printf("task_id=%s status=%s\n", task.ID, task.Status)
	fmt.Printf("context_object_id=%s kind=%s\n", object.ID, object.Kind)
	fmt.Println("snapshot_id=snapshot-initial")
	return nil
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "usage: prism run <goal>")
}
