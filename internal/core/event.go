package core

import "time"

type EventType string

const (
	EventRunCreated             EventType = "run.created"
	EventAgentCreated           EventType = "agent.created"
	EventConversationUserAdded  EventType = "conversation.user_appended"
	EventContextCollected       EventType = "context.collected"
	EventModelRequested         EventType = "model.requested"
	EventModelCompleted         EventType = "model.completed"
	EventConversationAgentAdded EventType = "conversation.agent_appended"
	EventRunResumed             EventType = "run.resumed"
	EventRunFailed              EventType = "run.failed"
	EventTaskCreated            EventType = "task.created"
	EventTaskStatusChanged      EventType = "task.status_changed"
	EventContextObjectCreated   EventType = "context_object.created"
	EventSnapshotCreated        EventType = "snapshot.created"
	EventSnapshotRestored       EventType = "snapshot.restored"
)

type Event struct {
	Type      EventType
	RunID     RunID
	TaskID    TaskID
	Timestamp time.Time
	Payload   map[string]string
}

func NewEvent(eventType EventType, runID RunID, taskID TaskID, payload map[string]string) Event {
	return Event{
		Type:      eventType,
		RunID:     runID,
		TaskID:    taskID,
		Timestamp: time.Now().UTC(),
		Payload:   payload,
	}
}
