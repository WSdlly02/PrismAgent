package core

import "time"

type Snapshot struct {
	ID        SnapshotID
	RunID     RunID
	Reason    string
	CreatedAt time.Time
}

type SnapshotState struct {
	Tasks          []Task
	ContextObjects []ContextObject
}

type SnapshotRecord struct {
	Snapshot Snapshot
	State    SnapshotState
}

func NewSnapshot(id SnapshotID, runID RunID, reason string) Snapshot {
	return Snapshot{
		ID:        id,
		RunID:     runID,
		Reason:    reason,
		CreatedAt: time.Now().UTC(),
	}
}
