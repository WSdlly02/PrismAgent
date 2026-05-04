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
