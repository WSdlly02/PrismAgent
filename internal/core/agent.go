package core

import "time"

type AgentRole string

const (
	AgentRoleRoot AgentRole = "root"
)

type AgentStatus string

const (
	AgentReady  AgentStatus = "READY"
	AgentActive AgentStatus = "ACTIVE"
	AgentDone   AgentStatus = "DONE"
	AgentFailed AgentStatus = "FAILED"
)

type Agent struct {
	ID        AgentID
	RunID     RunID
	ParentID  AgentID
	Role      AgentRole
	Status    AgentStatus
	CreatedAt time.Time
	UpdatedAt time.Time
}

func NewRootAgent(runID RunID) Agent {
	now := time.Now().UTC()
	return Agent{
		ID:        AgentID("agent-0"),
		RunID:     runID,
		Role:      AgentRoleRoot,
		Status:    AgentReady,
		CreatedAt: now,
		UpdatedAt: now,
	}
}
