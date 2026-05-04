package kernel

import (
	"fmt"
	"sync/atomic"
	"time"

	"prismagent/internal/core"
)

type ClockIDGenerator struct {
	counter atomic.Uint64
}

func NewClockIDGenerator() *ClockIDGenerator {
	return &ClockIDGenerator{}
}

func (g *ClockIDGenerator) WorkspaceID() core.WorkspaceID {
	return core.WorkspaceID(g.next("workspace"))
}

func (g *ClockIDGenerator) RunID() core.RunID {
	return core.RunID(g.next("run"))
}

func (g *ClockIDGenerator) TaskID() core.TaskID {
	return core.TaskID(g.next("task"))
}

func (g *ClockIDGenerator) ContextObjectID() core.ContextObjectID {
	return core.ContextObjectID(g.next("ctx"))
}

func (g *ClockIDGenerator) SnapshotID() core.SnapshotID {
	return core.SnapshotID(g.next("snapshot"))
}

func (g *ClockIDGenerator) next(prefix string) string {
	return fmt.Sprintf("%s-%d-%d", prefix, time.Now().UTC().UnixNano(), g.counter.Add(1))
}
