package core

import "time"

type AgentRole string

const (
	AgentRoleRoot AgentRole = "root"
	AgentRoleSub  AgentRole = "sub"
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
	Depth     int
	CreatedAt time.Time
	UpdatedAt time.Time
}

func NewRootAgent(runID RunID) Agent {
	now := time.Now().UTC()
	return Agent{
		ID:        AgentID("0"),
		RunID:     runID,
		Role:      AgentRoleRoot,
		Status:    AgentReady,
		Depth:     0,
		CreatedAt: now,
		UpdatedAt: now,
	}
}

func NewSubAgent(id AgentID, runID RunID, parentID AgentID, depth int) Agent {
	now := time.Now().UTC()
	return Agent{
		ID:        id,
		RunID:     runID,
		ParentID:  parentID,
		Role:      AgentRoleSub,
		Status:    AgentReady,
		Depth:     depth,
		CreatedAt: now,
		UpdatedAt: now,
	}
}
