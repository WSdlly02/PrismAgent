package agent

import (
	"context"

	"prismagent/internal/core"
)

type Planner interface {
	Plan(ctx context.Context, task core.Task, objects []core.ContextObject) ([]core.Task, error)
}

type Executor interface {
	Execute(ctx context.Context, task core.Task, objects []core.ContextObject) (ExecutionResult, error)
}

type Observer interface {
	Observe(ctx context.Context, task core.Task, result ExecutionResult) ([]core.ContextObject, error)
}

type Verifier interface {
	Verify(ctx context.Context, task core.Task, objects []core.ContextObject) (VerificationResult, error)
}

type Arbiter interface {
	Arbitrate(ctx context.Context, task core.Task, objects []core.ContextObject, err error) (ArbitrationDecision, error)
}

type ExecutionResult struct {
	Summary     string
	RawOutput   string
	ArtifactRef string
}

type VerificationResult struct {
	Passed bool
	Reason string
}

type ArbitrationDecision struct {
	Retry       bool
	Replan      bool
	FailTask    bool
	Explanation string
}
