package core

import (
	"fmt"
	"time"
)

type TaskStatus string

const (
	TaskReady   TaskStatus = "READY"
	TaskBlocked TaskStatus = "BLOCKED"
	TaskWaiting TaskStatus = "WAITING"
	TaskDone    TaskStatus = "DONE"
	TaskFailed  TaskStatus = "FAILED"
)

type Task struct {
	ID          TaskID
	RunID       RunID
	ParentID    TaskID
	Goal        string
	Description string
	Status      TaskStatus
	CreatedAt   time.Time
	UpdatedAt   time.Time
}

func NewTask(id TaskID, runID RunID, goal string) Task {
	now := time.Now().UTC()
	return Task{
		ID:        id,
		RunID:     runID,
		Goal:      goal,
		Status:    TaskReady,
		CreatedAt: now,
		UpdatedAt: now,
	}
}

func (t *Task) Transition(to TaskStatus, now time.Time) error {
	if !validTaskTransition(t.Status, to) {
		return fmt.Errorf("invalid task transition: %s -> %s", t.Status, to)
	}
	t.Status = to
	t.UpdatedAt = now.UTC()
	return nil
}

func validTaskTransition(from, to TaskStatus) bool {
	if from == to {
		return true
	}
	switch from {
	case TaskReady:
		return to == TaskBlocked || to == TaskWaiting || to == TaskDone || to == TaskFailed
	case TaskBlocked:
		return to == TaskReady || to == TaskFailed
	case TaskWaiting:
		return to == TaskReady || to == TaskDone || to == TaskFailed
	case TaskDone, TaskFailed:
		return false
	default:
		return false
	}
}
