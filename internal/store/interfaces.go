package store

import (
	"context"

	"prismagent/internal/core"
)

type TaskStore interface {
	CreateTask(ctx context.Context, task core.Task) error
	GetTask(ctx context.Context, id core.TaskID) (core.Task, error)
	UpdateTask(ctx context.Context, task core.Task) error
	ListTasks(ctx context.Context, runID core.RunID) ([]core.Task, error)
}

type ContextStore interface {
	PutContextObject(ctx context.Context, object core.ContextObject) error
	GetContextObject(ctx context.Context, id core.ContextObjectID) (core.ContextObject, error)
	ListContextObjects(ctx context.Context, scope core.ContextScope) ([]core.ContextObject, error)
}

type SnapshotStore interface {
	CreateSnapshot(ctx context.Context, snapshot core.Snapshot, state core.SnapshotState) error
	GetSnapshot(ctx context.Context, id core.SnapshotID) (core.SnapshotRecord, error)
	RestoreSnapshot(ctx context.Context, id core.SnapshotID) (core.SnapshotState, error)
}

type EventSink interface {
	Emit(ctx context.Context, event core.Event) error
}

type WorkspaceStore interface {
	InitWorkspace(ctx context.Context, workspace core.Workspace) error
	GetWorkspace(ctx context.Context) (core.Workspace, error)
}

type RunStore interface {
	CreateRun(ctx context.Context, run core.Run) error
	GetRun(ctx context.Context, id core.RunID) (core.Run, error)
	UpdateRun(ctx context.Context, run core.Run) error
	ListRuns(ctx context.Context) ([]core.Run, error)
	SetCurrentRun(ctx context.Context, id core.RunID) error
	GetCurrentRun(ctx context.Context) (core.RunID, error)
}

type AgentStore interface {
	WriteAgents(ctx context.Context, runID core.RunID, agents []core.Agent) error
	ListAgents(ctx context.Context, runID core.RunID) ([]core.Agent, error)
}

type ConversationStore interface {
	AppendConversationTurn(ctx context.Context, turn core.ConversationTurn) error
	ListConversationTurns(ctx context.Context, runID core.RunID) ([]core.ConversationTurn, error)
}

type RunArtifactStore interface {
	WriteRunArtifact(ctx context.Context, runID core.RunID, name string, body string) error
	ReadRunArtifact(ctx context.Context, runID core.RunID, name string) (string, error)
}
