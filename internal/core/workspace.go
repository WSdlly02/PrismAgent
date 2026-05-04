package core

import "time"

type Workspace struct {
	ID        WorkspaceID
	Root      string
	CreatedAt time.Time
	UpdatedAt time.Time
}

type RunStatus string

const (
	RunActive    RunStatus = "ACTIVE"
	RunCompleted RunStatus = "COMPLETED"
	RunFailed    RunStatus = "FAILED"
)

type Run struct {
	ID          RunID
	WorkspaceID WorkspaceID
	Goal        string
	Status      RunStatus
	CreatedAt   time.Time
	UpdatedAt   time.Time
}

func NewWorkspace(id WorkspaceID, root string) Workspace {
	now := time.Now().UTC()
	return Workspace{
		ID:        id,
		Root:      root,
		CreatedAt: now,
		UpdatedAt: now,
	}
}

func NewRun(id RunID, workspaceID WorkspaceID, goal string) Run {
	now := time.Now().UTC()
	return Run{
		ID:          id,
		WorkspaceID: workspaceID,
		Goal:        goal,
		Status:      RunActive,
		CreatedAt:   now,
		UpdatedAt:   now,
	}
}
