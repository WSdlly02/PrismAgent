package kernel

import (
	"context"
	"fmt"
	"strings"
	"time"

	"prismagent/internal/contextprovider"
	"prismagent/internal/core"
	"prismagent/internal/model"
	"prismagent/internal/store"
	"prismagent/internal/tool"
)

type Store interface {
	store.WorkspaceStore
	store.RunStore
	store.TaskStore
	store.ContextStore
	store.SnapshotStore
	store.EventSink
	store.AgentStore
	store.ConversationStore
	store.RunArtifactStore
}

type Kernel struct {
	store    Store
	ids      IDGenerator
	model    model.Client
	contexts contextprovider.Provider
	tools    *tool.Registry
}

type IDGenerator interface {
	WorkspaceID() core.WorkspaceID
	RunID() core.RunID
	TaskID() core.TaskID
	ContextObjectID() core.ContextObjectID
	SnapshotID() core.SnapshotID
}

type StartRunRequest struct {
	WorkspaceRoot string
	Goal          string
}

type StartRunResult struct {
	Workspace     core.Workspace
	Run           core.Run
	RootTask      core.Task
	InitialObject core.ContextObject
	Snapshot      core.Snapshot
}

func New(store Store, ids IDGenerator) *Kernel {
	return &Kernel{
		store:    store,
		ids:      ids,
		model:    model.MockClient{},
		contexts: contextprovider.NewLocalProvider("."),
		tools:    tool.NewRegistry(),
	}
}

func NewWithServices(store Store, ids IDGenerator, modelClient model.Client, contexts contextprovider.Provider) *Kernel {
	if modelClient == nil {
		modelClient = model.MockClient{}
	}
	if contexts == nil {
		contexts = contextprovider.NewLocalProvider(".")
	}
	return &Kernel{
		store:    store,
		ids:      ids,
		model:    modelClient,
		contexts: contexts,
		tools:    tool.NewRegistry(),
	}
}

func (k *Kernel) RegisterTool(tool tool.Tool) {
	k.tools.Register(tool)
}

func (k *Kernel) RegisterTools(tools ...tool.Tool) {
	for _, item := range tools {
		k.RegisterTool(item)
	}
}

type ToolCallRequest struct {
	RunID core.RunID
	Name  string
	Args  map[string]string
}

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

type NewRunRequest struct {
	WorkspaceRoot string
	Message       string
}

type NewRunResult struct {
	Workspace core.Workspace
	Run       core.Run
	Agent     core.Agent
	Turn      *RunTurnResult
}

type RunTurnRequest struct {
	WorkspaceRoot string
	RunID         core.RunID
	Message       string
}

type RunTurnResult struct {
	Run     core.Run
	Agent   core.Agent
	Context string
	Answer  string
}

type ResumeRunResult struct {
	Run          core.Run
	Agents       []core.Agent
	Conversation []core.ConversationTurn
	Answer       string
}

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

	userTurn := core.NewConversationTurn(req.RunID, agent.ID, core.ConversationUser, message)
	if err := k.store.AppendConversationTurn(ctx, userTurn); err != nil {
		return RunTurnResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventConversationUserAdded, req.RunID, "", map[string]string{
		"agent_id": agent.ID.String(),
	})); err != nil {
		return RunTurnResult{}, err
	}
	turns, err := k.store.ListConversationTurns(ctx, req.RunID)
	if err != nil {
		return RunTurnResult{}, err
	}

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
		"agent_id": agent.ID.String(),
	})); err != nil {
		return RunTurnResult{}, err
	}
	messages := buildModelMessages(turns, bundle.Text)
	response, err := k.model.Complete(ctx, model.Request{
		Model:    "default",
		Messages: messages,
		Metadata: map[string]string{
			"run_id":   req.RunID.String(),
			"agent_id": agent.ID.String(),
		},
	})
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

	agentTurn := core.NewConversationTurn(req.RunID, agent.ID, core.ConversationAgent, response.Text)
	if err := k.store.AppendConversationTurn(ctx, agentTurn); err != nil {
		return RunTurnResult{}, err
	}
	if err := k.store.WriteRunArtifact(ctx, req.RunID, "answer.md", response.Text); err != nil {
		return RunTurnResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventConversationAgentAdded, req.RunID, "", map[string]string{
		"agent_id": agent.ID.String(),
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

func (k *Kernel) ListRuns(ctx context.Context, workspaceRoot string) ([]core.Run, error) {
	if _, err := k.ensureWorkspace(ctx, workspaceRoot); err != nil {
		return nil, err
	}
	return k.store.ListRuns(ctx)
}

func (k *Kernel) CurrentRun(ctx context.Context, workspaceRoot string) (core.RunID, error) {
	if _, err := k.ensureWorkspace(ctx, workspaceRoot); err != nil {
		return "", err
	}
	return k.store.GetCurrentRun(ctx)
}

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
	turns, err := k.store.ListConversationTurns(ctx, runID)
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
		Run:          run,
		Agents:       agents,
		Conversation: turns,
		Answer:       answer,
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

func (k *Kernel) StartRun(ctx context.Context, req StartRunRequest) (StartRunResult, error) {
	goal := strings.TrimSpace(req.Goal)
	if goal == "" {
		return StartRunResult{}, fmt.Errorf("goal is required")
	}

	workspace := core.NewWorkspace(k.ids.WorkspaceID(), req.WorkspaceRoot)
	if err := k.store.InitWorkspace(ctx, workspace); err != nil {
		return StartRunResult{}, err
	}

	run := core.NewRun(k.ids.RunID(), workspace.ID, goal)
	if err := k.store.CreateRun(ctx, run); err != nil {
		return StartRunResult{}, err
	}

	task := core.NewTask(k.ids.TaskID(), run.ID, goal)
	if err := k.store.CreateTask(ctx, task); err != nil {
		return StartRunResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventTaskCreated, run.ID, task.ID, map[string]string{
		"goal": goal,
	})); err != nil {
		return StartRunResult{}, err
	}

	object := core.NewContextObject(k.ids.ContextObjectID(), core.ContextPlan, core.ContextScope{
		Type:        core.ScopeRun,
		WorkspaceID: workspace.ID,
		RunID:       run.ID,
	}, goal)
	object.Source = "user_goal"
	if err := k.store.PutContextObject(ctx, object); err != nil {
		return StartRunResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventContextObjectCreated, run.ID, task.ID, map[string]string{
		"context_object_id": object.ID.String(),
		"kind":              string(object.Kind),
	})); err != nil {
		return StartRunResult{}, err
	}

	snapshot := core.NewSnapshot(k.ids.SnapshotID(), run.ID, "initial run state")
	if err := k.store.CreateSnapshot(ctx, snapshot, core.SnapshotState{
		Tasks:          []core.Task{task},
		ContextObjects: []core.ContextObject{object},
	}); err != nil {
		return StartRunResult{}, err
	}
	if err := k.store.Emit(ctx, core.NewEvent(core.EventSnapshotCreated, run.ID, task.ID, map[string]string{
		"snapshot_id": snapshot.ID.String(),
		"reason":      snapshot.Reason,
	})); err != nil {
		return StartRunResult{}, err
	}

	return StartRunResult{
		Workspace:     workspace,
		Run:           run,
		RootTask:      task,
		InitialObject: object,
		Snapshot:      snapshot,
	}, nil
}

func buildModelMessages(turns []core.ConversationTurn, localContext string) []model.Message {
	messages := []model.Message{
		{
			Role:    "system",
			Content: "You are agent-0 in PrismAgent. Answer the user using the current run conversation and local workspace context. Be concise and explicit about uncertainty.",
		},
	}
	for _, turn := range turns {
		role := "user"
		if turn.Role == core.ConversationAgent {
			role = "assistant"
		}
		messages = append(messages, model.Message{
			Role:    role,
			Content: turn.Content,
		})
	}
	if strings.TrimSpace(localContext) != "" {
		messages = append(messages, model.Message{
			Role:    "user",
			Content: fmt.Sprintf("Local workspace context for this turn:\n%s", localContext),
		})
	}
	return messages
}
