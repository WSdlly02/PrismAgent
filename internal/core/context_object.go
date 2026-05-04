package core

import "time"

type ContextKind string

const (
	ContextFact        ContextKind = "Fact"
	ContextConstraint  ContextKind = "Constraint"
	ContextHypothesis  ContextKind = "Hypothesis"
	ContextPlan        ContextKind = "Plan"
	ContextObservation ContextKind = "Observation"
	ContextDecision    ContextKind = "Decision"
	ContextArtifact    ContextKind = "Artifact"
	ContextError       ContextKind = "Error"
)

type ContextScopeType string

const (
	ScopeWorkspace ContextScopeType = "workspace"
	ScopeRun       ContextScopeType = "run"
	ScopeTask      ContextScopeType = "task"
)

type ContextScope struct {
	Type        ContextScopeType
	WorkspaceID WorkspaceID
	RunID       RunID
	TaskID      TaskID
}

type ContextObject struct {
	ID          ContextObjectID
	Kind        ContextKind
	Scope       ContextScope
	Body        string
	Source      string
	Confidence  float64
	Tags        []string
	ArtifactRef string
	CreatedAt   time.Time
	UpdatedAt   time.Time
}

func NewContextObject(id ContextObjectID, kind ContextKind, scope ContextScope, body string) ContextObject {
	now := time.Now().UTC()
	return ContextObject{
		ID:         id,
		Kind:       kind,
		Scope:      scope,
		Body:       body,
		Confidence: 1,
		CreatedAt:  now,
		UpdatedAt:  now,
	}
}
