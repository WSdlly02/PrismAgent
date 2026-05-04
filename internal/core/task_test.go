package core

import (
	"testing"
	"time"
)

func TestTaskTransitionRejectsTerminalTransition(t *testing.T) {
	task := NewTask("task-1", "run-1", "test")
	if err := task.Transition(TaskDone, time.Now()); err != nil {
		t.Fatalf("transition to done failed: %v", err)
	}
	if err := task.Transition(TaskReady, time.Now()); err == nil {
		t.Fatal("expected terminal transition to be rejected")
	}
}
