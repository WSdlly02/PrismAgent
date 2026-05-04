package core

import "time"

type ConversationRole string

const (
	ConversationUser  ConversationRole = "user"
	ConversationAgent ConversationRole = "agent"
)

type ConversationTurn struct {
	RunID            RunID
	AgentID          AgentID
	Role             ConversationRole
	Content          string
	ReasoningContent string
	CreatedAt        time.Time
}

func NewConversationTurn(runID RunID, agentID AgentID, role ConversationRole, content string) ConversationTurn {
	return ConversationTurn{
		RunID:     runID,
		AgentID:   agentID,
		Role:      role,
		Content:   content,
		CreatedAt: time.Now().UTC(),
	}
}
