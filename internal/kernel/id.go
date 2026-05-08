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

func (g *ClockIDGenerator) next(prefix string) string {
	return fmt.Sprintf("%s-%d-%d", prefix, time.Now().UTC().UnixNano(), g.counter.Add(1))
}
