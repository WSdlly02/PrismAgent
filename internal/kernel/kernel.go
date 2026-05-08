package kernel

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"os"
	"strings"
	"time"

	"prismagent/internal/atom"
	"prismagent/internal/contextprovider"
	"prismagent/internal/core"
	"prismagent/internal/model"
	"prismagent/internal/tool"
	"prismagent/internal/unit"
)

const maxToolCallIterations = 64

// Store is the composite storage interface required by the Kernel.
// Implemented by filestore.Store.
type Store interface {
	// Workspace
	InitWorkspace(ctx context.Context, workspace core.Workspace) error
	GetWorkspace(ctx context.Context) (core.Workspace, error)

	// Run
	CreateRun(ctx context.Context, run core.Run) error
	GetRun(ctx context.Context, id core.RunID) (core.Run, error)
	UpdateRun(ctx context.Context, run core.Run) error
	ListRuns(ctx context.Context) ([]core.Run, error)
	SetCurrentRun(ctx context.Context, id core.RunID) error
	GetCurrentRun(ctx context.Context) (core.RunID, error)

	// Agent
	WriteAgents(ctx context.Context, runID core.RunID, agents []core.Agent) error
	AddAgent(ctx context.Context, runID core.RunID, agent core.Agent) error
	ListAgents(ctx context.Context, runID core.RunID) ([]core.Agent, error)

	// Artifacts
	WriteRunArtifact(ctx context.Context, runID core.RunID, name string, body string) error
	ReadRunArtifact(ctx context.Context, runID core.RunID, name string) (string, error)

	// Events
	Emit(ctx context.Context, event core.Event) error
}

// Kernel is the orchestration engine that wires together store, model,
// tools, atoms, and units to run the agent loop.
type Kernel struct {
	store         Store
	ids           IDGenerator
	model         model.Client
	contexts      contextprovider.Provider
	tools         *tool.Registry
	atomStore     *atom.Store
	unitStore     *unit.Store
	workspaceRoot string
	spawnTool     *spawnAgentTool
}

// IDGenerator produces unique identifiers for workspace entities.
type IDGenerator interface {
	WorkspaceID() core.WorkspaceID
	RunID() core.RunID
}

// New creates a Kernel with default (mock) model and local context provider.
func New(store Store, ids IDGenerator, workspaceRoot string) *Kernel {
	k := &Kernel{
		store:         store,
		ids:           ids,
		model:         model.MockClient{},
		contexts:      contextprovider.NewLocalProvider("."),
		tools:         tool.NewRegistry(),
		atomStore:     atom.NewStore(workspaceRoot),
		unitStore:     unit.NewStore(workspaceRoot),
		workspaceRoot: workspaceRoot,
	}
	k.spawnTool = newSpawnAgentTool(k)
	k.tools.Register(k.spawnTool)
	return k
}

// NewWithServices creates a Kernel with custom model, context provider, and workspace root.
func NewWithServices(store Store, ids IDGenerator, modelClient model.Client, contexts contextprovider.Provider, workspaceRoot string) *Kernel {
	if modelClient == nil {
		modelClient = model.MockClient{}
	}
	if contexts == nil {
		contexts = contextprovider.NewLocalProvider(".")
	}
	k := &Kernel{
		store:         store,
		ids:           ids,
		model:         modelClient,
		contexts:      contexts,
		tools:         tool.NewRegistry(),
		atomStore:     atom.NewStore(workspaceRoot),
		unitStore:     unit.NewStore(workspaceRoot),
		workspaceRoot: workspaceRoot,
	}
	k.spawnTool = newSpawnAgentTool(k)
	k.tools.Register(k.spawnTool)
	return k
}

func (k *Kernel) RegisterTool(t tool.Tool) {
	k.tools.Register(t)
}

func (k *Kernel) RegisterTools(tools ...tool.Tool) {
	for _, t := range tools {
		k.RegisterTool(t)
	}
}

// ToolCallRequest is a request to execute a named tool.
type ToolCallRequest struct {
	RunID core.RunID
	Name  string
	Args  map[string]string
}

// CallTool executes a tool by name and emits lifecycle events.
func (k *Kernel) CallTool(ctx context.Context, req ToolCallRequest) (tool.Result, error) {
	if strings.TrimSpace(req.Name) == "" {
		return tool.Result{}, fmt.Errorf("tool name is required")
	}
	if _, err := k.store.GetRun(ctx, req.RunID); err != nil {
		return tool.Result{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventToolRequested, req.RunID, "", map[string]string{
		"tool": req.Name,
	})); err != nil {
		return tool.Result{}, err
	}
	result, err := k.tools.Call(ctx, tool.Request{
		Name: req.Name,
		Args: req.Args,
	})
	if err != nil {
		_ = k.store.Emit(ctx, core.NewEvent(core.EventToolFailed, req.RunID, "", map[string]string{
			"tool":  req.Name,
			"error": err.Error(),
		}))
		return tool.Result{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventToolCompleted, req.RunID, "", map[string]string{
		"tool":    req.Name,
		"summary": result.Summary,
	})); err != nil {
		return tool.Result{}, err
	}
	return result, nil
}

// NewRunRequest contains the parameters for creating a new run.
type NewRunRequest struct {
	WorkspaceRoot string
	Message       string
}

// NewRunResult is the result of creating a new run.
type NewRunResult struct {
	Workspace core.Workspace
	Run       core.Run
	Agent     core.Agent
	Turn      *RunTurnResult
}

// RunTurnRequest contains the parameters for sending a message to a run.
type RunTurnRequest struct {
	WorkspaceRoot string
	RunID         core.RunID
	Message       string
}

// RunTurnResult is the result of processing a message turn.
type RunTurnResult struct {
	Run     core.Run
	Agent   core.Agent
	Context string
	Answer  string
}

// ResumeRunResult is the result of resuming a previous run.
type ResumeRunResult struct {
	Run    core.Run
	Agents []core.Agent
	Answer string
}

// NewRun creates a new run with a root agent and optionally processes an initial message.
func (k *Kernel) NewRun(ctx context.Context, req NewRunRequest) (NewRunResult, error) {
	workspace, err := k.ensureWorkspace(ctx, req.WorkspaceRoot)
	if err != nil {
		return NewRunResult{}, err
	}

	title := strings.TrimSpace(req.Message)
	if title == "" {
		title = "Untitled run"
	}
	run := core.NewRun(k.ids.RunID(), workspace.ID, title)
	if err := k.store.CreateRun(ctx, run); err != nil {
		return NewRunResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventRunCreated, run.ID, "", map[string]string{
		"goal": run.Goal,
	})); err != nil {
		return NewRunResult{}, err
	}

	agent := core.NewRootAgent(run.ID)
	if err := k.store.WriteAgents(ctx, run.ID, []core.Agent{agent}); err != nil {
		return NewRunResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventAgentCreated, run.ID, "", map[string]string{
		"agent_id": agent.ID.String(),
		"role":     string(agent.Role),
	})); err != nil {
		return NewRunResult{}, err
	}
	if err := k.store.SetCurrentRun(ctx, run.ID); err != nil {
		return NewRunResult{}, err
	}

	result := NewRunResult{
		Workspace: workspace,
		Run:       run,
		Agent:     agent,
	}
	if strings.TrimSpace(req.Message) != "" {
		turn, err := k.RunMessage(ctx, RunTurnRequest{
			WorkspaceRoot: req.WorkspaceRoot,
			RunID:         run.ID,
			Message:       req.Message,
		})
		if err != nil {
			return NewRunResult{}, err
		}
		result.Turn = &turn
	}
	return result, nil
}

// RunMessage processes a user message within an existing run.
func (k *Kernel) RunMessage(ctx context.Context, req RunTurnRequest) (RunTurnResult, error) {
	message := strings.TrimSpace(req.Message)
	if message == "" {
		return RunTurnResult{}, fmt.Errorf("message is required")
	}
	if _, err := k.ensureWorkspace(ctx, req.WorkspaceRoot); err != nil {
		return RunTurnResult{}, err
	}

	run, err := k.store.GetRun(ctx, req.RunID)
	if err != nil {
		return RunTurnResult{}, err
	}
	agents, err := k.store.ListAgents(ctx, req.RunID)
	if err != nil {
		return RunTurnResult{}, err
	}
	if len(agents) == 0 {
		return RunTurnResult{}, fmt.Errorf("run has no root agent: %s", req.RunID)
	}
	agent := agents[0]
	agentID := agent.ID.String()

	// Load the agent chain
	chain, err := unit.LoadChain(req.WorkspaceRoot, string(req.RunID), agentID)
	if err != nil {
		return RunTurnResult{}, fmt.Errorf("load chain: %w", err)
	}

	// Ensure system prompt exists in chain
	hasSystem := false
	for _, uuid := range chain.Chain {
		u, err := k.unitStore.Get(string(req.RunID), uuid)
		if err == nil && u.Kind == core.UnitMessage && u.Role == core.RoleSystem {
			hasSystem = true
			break
		}
	}
	if !hasSystem {
		systemAtomHash, err := k.atomStore.Put(fmt.Appendf(nil, "You are agent %s in PrismAgent. Answer the user using the current run conversation and local workspace context. Be concise and explicit about uncertainty.", agentID))
		if err != nil {
			return RunTurnResult{}, fmt.Errorf("put system atom: %w", err)
		}
		systemUnit := core.Unit{
			UUID:       newUnitID(),
			AtomHash:   systemAtomHash,
			Kind:       core.UnitMessage,
			Role:       core.RoleSystem,
			Scope:      core.ScopeAgent,
			Visibility: core.VisibilityInternal,
			CreatedAt:  time.Now().UTC(),
		}
		if err := k.unitStore.Put(string(req.RunID), systemUnit); err != nil {
			return RunTurnResult{}, fmt.Errorf("put system unit: %w", err)
		}
		if err := unit.AppendToChain(req.WorkspaceRoot, string(req.RunID), agentID, systemUnit.UUID); err != nil {
			return RunTurnResult{}, fmt.Errorf("append system to chain: %w", err)
		}
	}

	// Create user message atom + unit
	userAtomHash, err := k.atomStore.Put([]byte(message))
	if err != nil {
		return RunTurnResult{}, fmt.Errorf("put user atom: %w", err)
	}
	userUnit := core.Unit{
		UUID:       newUnitID(),
		AtomHash:   userAtomHash,
		Kind:       core.UnitMessage,
		Role:       core.RoleUser,
		Scope:      core.ScopeAgent,
		Visibility: core.VisibilityUser,
		CreatedAt:  time.Now().UTC(),
	}
	if err := k.unitStore.Put(string(req.RunID), userUnit); err != nil {
		return RunTurnResult{}, fmt.Errorf("put user unit: %w", err)
	}
	if err := unit.AppendToChain(req.WorkspaceRoot, string(req.RunID), agentID, userUnit.UUID); err != nil {
		return RunTurnResult{}, fmt.Errorf("append user to chain: %w", err)
	}

	if err := k.store.Emit(ctx, core.NewEvent(core.EventConversationUserAdded, req.RunID, "", map[string]string{
		"agent_id": agentID,
	})); err != nil {
		return RunTurnResult{}, err
	}

	// Collect local context
	bundle, err := k.contexts.Collect(ctx, message)
	if err != nil {
		return RunTurnResult{}, err
	}
	if err := k.store.WriteRunArtifact(ctx, req.RunID, "context.md", bundle.Text); err != nil {
		return RunTurnResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventContextCollected, req.RunID, "", map[string]string{
		"files": fmt.Sprintf("%d", len(bundle.Files)),
	})); err != nil {
		return RunTurnResult{}, err
	}

	if err := k.store.Emit(ctx, core.NewEvent(core.EventModelRequested, req.RunID, "", map[string]string{
		"agent_id": agentID,
	})); err != nil {
		return RunTurnResult{}, err
	}

	// Reload chain after appending user unit
	chain, err = unit.LoadChain(req.WorkspaceRoot, string(req.RunID), agentID)
	if err != nil {
		return RunTurnResult{}, fmt.Errorf("reload chain: %w", err)
	}
	units, err := k.unitStore.List(string(req.RunID), chain.Chain)
	if err != nil {
		return RunTurnResult{}, fmt.Errorf("list units: %w", err)
	}
	messages, err := unit.AssembleMessages(units, k.atomStore)
	if err != nil {
		return RunTurnResult{}, fmt.Errorf("assemble messages: %w", err)
	}

	// Append local context as ephemeral user message (not persisted in chain)
	if strings.TrimSpace(bundle.Text) != "" {
		messages = append(messages, model.Message{
			Role:    "user",
			Content: fmt.Sprintf("Local workspace context for this turn:\n%s", bundle.Text),
		})
	}

	// Run the LLM tool loop
	response, err := k.completeWithToolLoop(ctx, req.RunID, agentID, req.WorkspaceRoot, messages)
	if err != nil {
		_ = k.store.Emit(ctx, core.NewEvent(core.EventRunFailed, req.RunID, "", map[string]string{
			"error": err.Error(),
		}))
		return RunTurnResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventModelCompleted, req.RunID, "", map[string]string{
		"model": response.Model,
	})); err != nil {
		return RunTurnResult{}, err
	}

	if err := k.store.WriteRunArtifact(ctx, req.RunID, "answer.md", response.Text); err != nil {
		return RunTurnResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventConversationAgentAdded, req.RunID, "", map[string]string{
		"agent_id": agentID,
	})); err != nil {
		return RunTurnResult{}, err
	}

	run.UpdatedAt = time.Now().UTC()
	if err := k.store.UpdateRun(ctx, run); err != nil {
		return RunTurnResult{}, err
	}

	return RunTurnResult{
		Run:     run,
		Agent:   agent,
		Context: bundle.Text,
		Answer:  response.Text,
	}, nil
}

// ListRuns returns all runs in the workspace.
func (k *Kernel) ListRuns(ctx context.Context, workspaceRoot string) ([]core.Run, error) {
	if _, err := k.ensureWorkspace(ctx, workspaceRoot); err != nil {
		return nil, err
	}
	return k.store.ListRuns(ctx)
}

// CurrentRun returns the ID of the most recently active run.
func (k *Kernel) CurrentRun(ctx context.Context, workspaceRoot string) (core.RunID, error) {
	if _, err := k.ensureWorkspace(ctx, workspaceRoot); err != nil {
		return "", err
	}
	return k.store.GetCurrentRun(ctx)
}

// ResumeRun restores a previous run's state and sets it as current.
func (k *Kernel) ResumeRun(ctx context.Context, workspaceRoot string, runID core.RunID) (ResumeRunResult, error) {
	if _, err := k.ensureWorkspace(ctx, workspaceRoot); err != nil {
		return ResumeRunResult{}, err
	}
	run, err := k.store.GetRun(ctx, runID)
	if err != nil {
		return ResumeRunResult{}, err
	}
	agents, err := k.store.ListAgents(ctx, runID)
	if err != nil {
		return ResumeRunResult{}, err
	}
	answer, err := k.store.ReadRunArtifact(ctx, runID, "answer.md")
	if err != nil {
		answer = ""
	}
	if err := k.store.SetCurrentRun(ctx, runID); err != nil {
		return ResumeRunResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventRunResumed, runID, "", nil)); err != nil {
		return ResumeRunResult{}, err
	}
	return ResumeRunResult{
		Run:    run,
		Agents: agents,
		Answer: answer,
	}, nil
}

func (k *Kernel) ensureWorkspace(ctx context.Context, root string) (core.Workspace, error) {
	workspace, err := k.store.GetWorkspace(ctx)
	if err == nil {
		return workspace, nil
	}
	workspace = core.NewWorkspace(k.ids.WorkspaceID(), root)
	if err := k.store.InitWorkspace(ctx, workspace); err != nil {
		return core.Workspace{}, err
	}
	return workspace, nil
}

// completeWithToolLoop runs the LLM with tool-calling support.
// Each LLM response is persisted as an Atom + Unit. Tool calls and results
// are also persisted as Atoms + Units and appended to the agent chain.
func (k *Kernel) completeWithToolLoop(ctx context.Context, runID core.RunID, agentID string, workspaceRoot string, messages []model.Message) (model.Response, error) {
	tools := k.modelToolDefinitions()
	var finalResponse model.Response

	for iteration := range maxToolCallIterations {
		response, err := k.model.Complete(ctx, model.Request{
			Model:    "default",
			Messages: messages,
			Tools:    tools,
			Metadata: map[string]string{
				"run_id":   runID.String(),
				"agent_id": agentID,
			},
		})
		if err != nil {
			return model.Response{}, err
		}

		// Persist the LLM response as atom + unit.
		// Prefer raw provider response JSON; fall back to synthesized format for mocks.
		var llmAtomBytes []byte
		if len(response.RawPayload) > 0 {
			llmAtomBytes = response.RawPayload
		} else {
			llmAtomBytes = unit.BuildLLMAtom(response)
		}
		llmAtomHash, err := k.atomStore.Put(llmAtomBytes)
		if err != nil {
			return model.Response{}, fmt.Errorf("put llm atom: %w", err)
		}
		llmUnit := core.Unit{
			UUID:       newUnitID(),
			AtomHash:   llmAtomHash,
			Kind:       core.UnitLLMResp,
			Role:       core.RoleAssistant,
			Scope:      core.ScopeAgent,
			Visibility: core.VisibilityInternal,
			Metadata:   map[string]string{"iteration": fmt.Sprintf("%d", iteration)},
			CreatedAt:  time.Now().UTC(),
		}
		if err := k.unitStore.Put(string(runID), llmUnit); err != nil {
			return model.Response{}, fmt.Errorf("put llm unit: %w", err)
		}
		if err := unit.AppendToChain(workspaceRoot, string(runID), agentID, llmUnit.UUID); err != nil {
			return model.Response{}, fmt.Errorf("append llm to chain: %w", err)
		}

		// Parse the response atom to extract tool calls
		toolCalls := unit.ParseToolCalls(llmAtomBytes)
		if len(toolCalls) == 0 {
			finalResponse = response
			break
		}

		fmt.Fprintf(os.Stderr, "[tool-loop] iteration=%d agent=%s tool_calls=%d\n", iteration, agentID, len(toolCalls))

		// Process each tool call
		for _, tc := range toolCalls {
			fmt.Fprintf(os.Stderr, "  [tool-call] %s(%s)\n", tc.Name, summarizeArgs(tc.Arguments))

			// Audit unit for the tool call
			callUnit := core.Unit{
				UUID:       newUnitID(),
				AtomHash:   llmAtomHash,
				Kind:       core.UnitToolCall,
				Role:       core.RoleAssistant,
				Scope:      core.ScopeAgent,
				Visibility: core.VisibilityInternal,
				Metadata: map[string]string{
					"tool_call_id": tc.ID,
					"tool_name":    tc.Name,
				},
				CreatedAt: time.Now().UTC(),
			}
			if err := k.unitStore.Put(string(runID), callUnit); err != nil {
				return model.Response{}, fmt.Errorf("put tool call unit: %w", err)
			}
			if err := unit.AppendToChain(workspaceRoot, string(runID), agentID, callUnit.UUID); err != nil {
				return model.Response{}, fmt.Errorf("append tool call to chain: %w", err)
			}

			// Execute the tool — inject agent context for spawn_agent
			args := tc.Arguments
			isSpawn := tc.Name == "spawn_agent"
			if isSpawn {
				if args == nil {
					args = make(map[string]string)
				}
				args["_run_id"] = string(runID)
				args["_agent_id"] = agentID
				args["_workspace_root"] = workspaceRoot
				agentDepth := k.getAgentDepth(ctx, runID, agentID)
				args["_agent_depth"] = fmt.Sprintf("%d", agentDepth)

				// Record spawn Unit in parent chain before execution
				spawnAtomHash, putErr := k.atomStore.Put([]byte(args["message"]))
				if putErr != nil {
					return model.Response{}, fmt.Errorf("put spawn atom: %w", putErr)
				}
				spawnUnit := core.Unit{
					UUID:       newUnitID(),
					AtomHash:   spawnAtomHash,
					Kind:       core.UnitSpawn,
					Role:       core.RoleAssistant,
					Scope:      core.ScopeAgent,
					Visibility: core.VisibilityInternal,
					Metadata: map[string]string{
						"tool_call_id": tc.ID,
						"goal":         args["message"],
					},
					CreatedAt: time.Now().UTC(),
				}
				if err := k.unitStore.Put(string(runID), spawnUnit); err != nil {
					return model.Response{}, fmt.Errorf("put spawn unit: %w", err)
				}
				if err := unit.AppendToChain(workspaceRoot, string(runID), agentID, spawnUnit.UUID); err != nil {
					return model.Response{}, fmt.Errorf("append spawn to chain: %w", err)
				}
			}
			toolResult, err := k.CallTool(ctx, ToolCallRequest{
				RunID: runID,
				Name:  tc.Name,
				Args:  args,
			})
			toolOutput := toolResult.RawOutput
			if err != nil {
				toolOutput = fmt.Sprintf("tool %s failed: %s", tc.Name, err.Error())
				fmt.Fprintf(os.Stderr, "  [tool-result] %s -> FAILED: %s\n", tc.Name, err.Error())
			} else {
				fmt.Fprintf(os.Stderr, "  [tool-result] %s -> ok (%s)\n", tc.Name, toolResult.Summary)
			}

			// Record result Unit in parent chain after spawn_agent completes
			if isSpawn {
				status := "DONE"
				if err != nil {
					status = "FAILED"
				}
				resultAtomHash, putErr := k.atomStore.Put([]byte(toolOutput))
				if putErr != nil {
					return model.Response{}, fmt.Errorf("put spawn result atom: %w", putErr)
				}
				spawnResultUnit := core.Unit{
					UUID:       newUnitID(),
					AtomHash:   resultAtomHash,
					Kind:       core.UnitResult,
					Role:       core.RoleTool,
					Scope:      core.ScopeAgent,
					Visibility: core.VisibilityInternal,
					Metadata: map[string]string{
						"tool_call_id": tc.ID,
						"status":       status,
					},
					CreatedAt: time.Now().UTC(),
				}
				if err := k.unitStore.Put(string(runID), spawnResultUnit); err != nil {
					return model.Response{}, fmt.Errorf("put spawn result unit: %w", err)
				}
				if err := unit.AppendToChain(workspaceRoot, string(runID), agentID, spawnResultUnit.UUID); err != nil {
					return model.Response{}, fmt.Errorf("append spawn result to chain: %w", err)
				}
			}

			// Persist tool result as atom + unit
			resultAtomHash, err := k.atomStore.Put([]byte(toolOutput))
			if err != nil {
				return model.Response{}, fmt.Errorf("put tool result atom: %w", err)
			}
			resultUnit := core.Unit{
				UUID:       newUnitID(),
				AtomHash:   resultAtomHash,
				Kind:       core.UnitToolResult,
				Role:       core.RoleTool,
				Scope:      core.ScopeAgent,
				Visibility: core.VisibilityInternal,
				Metadata: map[string]string{
					"tool_call_id": tc.ID,
					"tool_name":    tc.Name,
				},
				CreatedAt: time.Now().UTC(),
			}
			if err := k.unitStore.Put(string(runID), resultUnit); err != nil {
				return model.Response{}, fmt.Errorf("put tool result unit: %w", err)
			}
			if err := unit.AppendToChain(workspaceRoot, string(runID), agentID, resultUnit.UUID); err != nil {
				return model.Response{}, fmt.Errorf("append tool result to chain: %w", err)
			}

			messages = append(messages, model.Message{
				Role:       "tool",
				ToolCallID: tc.ID,
				Content:    toolOutput,
			})
		}

		// Reload chain for next iteration
		chain, err := unit.LoadChain(workspaceRoot, string(runID), agentID)
		if err != nil {
			return model.Response{}, fmt.Errorf("reload chain: %w", err)
		}
		units, err := k.unitStore.List(string(runID), chain.Chain)
		if err != nil {
			return model.Response{}, fmt.Errorf("list units: %w", err)
		}
		assembled, err := unit.AssembleMessages(units, k.atomStore)
		if err != nil {
			return model.Response{}, fmt.Errorf("assemble messages: %w", err)
		}
		messages = assembled
	}

	return finalResponse, nil
}

func (k *Kernel) modelToolDefinitions() []model.ToolDefinition {
	definitions := k.tools.Definitions()
	modelDefinitions := make([]model.ToolDefinition, 0, len(definitions))
	for _, definition := range definitions {
		modelDefinitions = append(modelDefinitions, model.ToolDefinition{
			Name:        definition.Name,
			Description: definition.Description,
			Parameters:  definition.Parameters,
		})
	}
	return modelDefinitions
}

// newUnitID generates a unique ID for a Unit.
func newUnitID() string {
	b := make([]byte, 8)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

func summarizeArgs(args map[string]string) string {
	if len(args) == 0 {
		return ""
	}
	longFields := map[string]bool{"content": true, "old": true, "new": true}
	parts := make([]string, 0, len(args))
	if v, ok := args["path"]; ok {
		parts = append(parts, v)
	}
	for k, v := range args {
		if k == "path" {
			continue
		}
		if longFields[k] {
			short := v
			if len(short) > 40 {
				short = short[:40] + "..."
			}
			parts = append(parts, k+"="+short)
		} else {
			parts = append(parts, k+"="+v)
		}
	}
	return strings.Join(parts, ", ")
}

// parseToolCallsFromAtom is a legacy helper kept for spawn.go compatibility.
// Prefer unit.ParseToolCalls for new code.
func parseToolCallsFromAtom(data []byte) []struct {
	ID   string
	Name string
	Args map[string]string
} {
	calls := unit.ParseToolCalls(data)
	result := make([]struct {
		ID   string
		Name string
		Args map[string]string
	}, 0, len(calls))
	for _, tc := range calls {
		result = append(result, struct {
			ID   string
			Name string
			Args map[string]string
		}{ID: tc.ID, Name: tc.Name, Args: tc.Arguments})
	}
	return result
}
