package kernel

import (
	"context"
	"fmt"
	"sync/atomic"

	"prismagent/internal/core"
	"prismagent/internal/tool"
	"prismagent/internal/unit"
)

const maxSpawnDepth = 4

// spawnAgentTool implements the spawn_agent tool that allows the model
// to delegate work to a sub-agent with its own conversation chain.
type spawnAgentTool struct {
	kernel       *Kernel
	agentCounter *atomic.Int64
}

func newSpawnAgentTool(k *Kernel) *spawnAgentTool {
	return &spawnAgentTool{
		kernel:       k,
		agentCounter: &atomic.Int64{},
	}
}

func (t *spawnAgentTool) Name() string { return "spawn_agent" }

func (t *spawnAgentTool) Definition() tool.Definition {
	return tool.Definition{
		Name:        "spawn_agent",
		Description: "Spawn a sub-agent to perform a task independently. The sub-agent has its own conversation context and can use all available tools. Use this to parallelize independent subtasks or to isolate context for a specific problem.",
		Parameters: map[string]any{
			"type": "object",
			"properties": map[string]any{
				"message": map[string]any{
					"type":        "string",
					"description": "The task description or message for the sub-agent to work on.",
				},
			},
			"required": []string{"message"},
		},
	}
}

func (t *spawnAgentTool) Call(ctx context.Context, req tool.Request) (tool.Result, error) {
	message := req.Args["message"]
	if message == "" {
		return tool.Result{}, fmt.Errorf("spawn_agent: message is required")
	}

	// Extract parent agent context from metadata
	runID := core.RunID(req.Args["_run_id"])
	parentID := req.Args["_agent_id"]
	parentDepth := parseIntArg(req.Args["_agent_depth"], 0)

	if parentDepth >= maxSpawnDepth {
		return tool.Result{}, fmt.Errorf("spawn_agent denied: maximum depth (%d) reached", maxSpawnDepth)
	}

	childID := fmt.Sprintf("%d", t.agentCounter.Add(1))
	childAgent := core.NewSubAgent(
		core.AgentID(childID),
		runID,
		core.AgentID(parentID),
		parentDepth+1,
	)

	// Persist the child agent
	if err := t.kernel.store.AddAgent(ctx, runID, childAgent); err != nil {
		return tool.Result{}, fmt.Errorf("spawn_agent: add agent: %w", err)
	}

	// Run the sub-agent
	answer, err := t.kernel.runSubAgent(ctx, subAgentRequest{
		RunID:      runID,
		AgentID:    childID,
		ParentID:   parentID,
		Depth:      parentDepth + 1,
		Root:       req.Args["_workspace_root"],
		Message:    message,
	})
	if err != nil {
		return tool.Result{
			RawOutput: fmt.Sprintf("spawn_agent failed: %s", err.Error()),
			Summary:   "sub-agent failed",
		}, nil
	}

	return tool.Result{
		RawOutput: answer,
		Summary:   fmt.Sprintf("agent %s completed", childID),
	}, nil
}

type subAgentRequest struct {
	RunID    core.RunID
	AgentID  string
	ParentID string
	Depth    int
	Root     string
	Message  string
}

// runSubAgent executes a sub-agent's LLM loop in isolation.
// It creates its own chain with system + user messages and runs completeWithToolLoop.
func (k *Kernel) runSubAgent(ctx context.Context, req subAgentRequest) (string, error) {
	runID := string(req.RunID)

	// System prompt for sub-agent
	systemAtomHash, err := k.atomStore.Put(fmt.Appendf(nil,
		"You are sub-agent %s spawned by agent %s. Complete the assigned task using the available tools. Be concise and thorough.",
		req.AgentID, req.ParentID,
	))
	if err != nil {
		return "", fmt.Errorf("sub-agent: put system atom: %w", err)
	}
	systemUnit := core.Unit{
		UUID:       newUnitID(),
		AtomHash:   systemAtomHash,
		Kind:       core.UnitMessage,
		Role:       core.RoleSystem,
		Scope:      core.ScopeAgent,
		Visibility: core.VisibilityInternal,
		Metadata:   map[string]string{"agent_id": req.AgentID},
	}
	if err := k.unitStore.Put(runID, systemUnit); err != nil {
		return "", fmt.Errorf("sub-agent: put system unit: %w", err)
	}

	// User message
	userAtomHash, err := k.atomStore.Put([]byte(req.Message))
	if err != nil {
		return "", fmt.Errorf("sub-agent: put user atom: %w", err)
	}
	userUnit := core.Unit{
		UUID:       newUnitID(),
		AtomHash:   userAtomHash,
		Kind:       core.UnitMessage,
		Role:       core.RoleUser,
		Scope:      core.ScopeAgent,
		Visibility: core.VisibilityUser,
		Metadata:   map[string]string{"agent_id": req.AgentID},
	}
	if err := k.unitStore.Put(runID, userUnit); err != nil {
		return "", fmt.Errorf("sub-agent: put user unit: %w", err)
	}

	// Initialize sub-agent chain
	subChain := core.AgentChain{
		AgentID:  req.AgentID,
		Chain:    []string{systemUnit.UUID, userUnit.UUID},
		Head:     userUnit.UUID,
		Children: []string{},
	}
	if err := unit.SaveChain(req.Root, runID, subChain); err != nil {
		return "", fmt.Errorf("sub-agent: save chain: %w", err)
	}

	// Assemble messages
	units, err := k.unitStore.List(runID, subChain.Chain)
	if err != nil {
		return "", fmt.Errorf("sub-agent: list units: %w", err)
	}
	messages, err := unit.AssembleMessages(units, k.atomStore)
	if err != nil {
		return "", fmt.Errorf("sub-agent: assemble: %w", err)
	}

	// Run the LLM tool loop
	response, err := k.completeWithToolLoop(ctx, req.RunID, req.AgentID, req.Root, messages)
	if err != nil {
		return "", fmt.Errorf("sub-agent: tool loop: %w", err)
	}

	return response.Text, nil
}

// getAgentDepth returns the depth of the agent with the given ID.
func (k *Kernel) getAgentDepth(ctx context.Context, runID core.RunID, agentID string) int {
	agents, err := k.store.ListAgents(ctx, runID)
	if err != nil {
		return 0
	}
	for _, a := range agents {
		if a.ID.String() == agentID {
			return a.Depth
		}
	}
	return 0
}

func parseIntArg(s string, defaultVal int) int {
	if s == "" {
		return defaultVal
	}
	var n int
	fmt.Sscanf(s, "%d", &n)
	return n
}
