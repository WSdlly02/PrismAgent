package main

import (
	"context"
	"fmt"
	"os"
	"strings"

	"prismagent/internal/contextprovider"
	"prismagent/internal/core"
	"prismagent/internal/filestore"
	"prismagent/internal/kernel"
	"prismagent/internal/model"
)

func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(2)
	}

	if err := dispatch(os.Args[1], os.Args[2:]); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func dispatch(command string, args []string) error {
	ctx := context.Background()
	root, err := os.Getwd()
	if err != nil {
		return err
	}
	k := kernel.NewWithServices(
		filestore.New(root),
		kernel.NewClockIDGenerator(),
		model.MockClient{},
		contextprovider.NewLocalProvider(root),
	)

	switch command {
	case "list":
		return listRuns(ctx, k, root)
	case "new":
		return newRun(ctx, k, root, strings.Join(args, " "))
	case "run":
		if len(args) < 2 {
			return fmt.Errorf("usage: prismagent run <run_id> <message>")
		}
		return runMessage(ctx, k, root, core.RunID(args[0]), strings.Join(args[1:], " "))
	case "resume":
		if len(args) != 1 {
			return fmt.Errorf("usage: prismagent resume <run_id>")
		}
		return resumeRun(ctx, k, root, core.RunID(args[0]))
	default:
		printUsage()
		return fmt.Errorf("unknown command: %s", command)
	}
}

func listRuns(ctx context.Context, k *kernel.Kernel, root string) error {
	runs, err := k.ListRuns(ctx, root)
	if err != nil {
		return err
	}
	if len(runs) == 0 {
		fmt.Println("no runs")
		return nil
	}
	for _, run := range runs {
		fmt.Printf("%s\t%s\t%s\t%s\n", run.ID, run.Status, run.UpdatedAt.Format("2006-01-02 15:04:05"), run.Goal)
	}
	return nil
}

func newRun(ctx context.Context, k *kernel.Kernel, root string, message string) error {
	result, err := k.NewRun(ctx, kernel.NewRunRequest{
		WorkspaceRoot: root,
		Message:       message,
	})
	if err != nil {
		return err
	}
	fmt.Printf("run_id=%s\n", result.Run.ID)
	fmt.Printf("agent_id=%s\n", result.Agent.ID)
	if result.Turn != nil {
		fmt.Println()
		fmt.Println(result.Turn.Answer)
	}
	return nil
}

func runMessage(ctx context.Context, k *kernel.Kernel, root string, runID core.RunID, message string) error {
	result, err := k.RunMessage(ctx, kernel.RunTurnRequest{
		WorkspaceRoot: root,
		RunID:         runID,
		Message:       message,
	})
	if err != nil {
		return err
	}
	fmt.Println(result.Answer)
	return nil
}

func resumeRun(ctx context.Context, k *kernel.Kernel, root string, runID core.RunID) error {
	result, err := k.ResumeRun(ctx, root, runID)
	if err != nil {
		return err
	}
	fmt.Printf("run_id=%s\n", result.Run.ID)
	fmt.Printf("status=%s\n", result.Run.Status)
	fmt.Printf("goal=%s\n", result.Run.Goal)
	fmt.Printf("agents=%d\n", len(result.Agents))
	if len(result.Conversation) > 0 {
		fmt.Println()
		fmt.Println("conversation:")
		start := len(result.Conversation) - 6
		if start < 0 {
			start = 0
		}
		for _, turn := range result.Conversation[start:] {
			fmt.Printf("[%s] %s\n", turn.Role, turn.Content)
		}
	}
	return nil
}

func printUsage() {
	fmt.Fprintln(os.Stderr, "usage:")
	fmt.Fprintln(os.Stderr, "  prismagent list")
	fmt.Fprintln(os.Stderr, "  prismagent new [message]")
	fmt.Fprintln(os.Stderr, "  prismagent run <run_id> <message>")
	fmt.Fprintln(os.Stderr, "  prismagent resume <run_id>")
}
