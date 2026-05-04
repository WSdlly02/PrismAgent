package core

import "time"

type Budget struct {
	MaxTokens    int
	MaxToolCalls int
	MaxErrors    int
	MaxDuration  time.Duration

	TokensUsed    int
	ToolCallsUsed int
	ErrorsUsed    int
	StartedAt     time.Time
}

func NewBudget(maxTokens, maxToolCalls, maxErrors int, maxDuration time.Duration) Budget {
	return Budget{
		MaxTokens:    maxTokens,
		MaxToolCalls: maxToolCalls,
		MaxErrors:    maxErrors,
		MaxDuration:  maxDuration,
		StartedAt:    time.Now().UTC(),
	}
}

func (b *Budget) ConsumeTokens(tokens int) bool {
	if tokens < 0 {
		return false
	}
	if b.MaxTokens > 0 && b.TokensUsed+tokens > b.MaxTokens {
		return false
	}
	b.TokensUsed += tokens
	return true
}

func (b *Budget) ConsumeToolCall() bool {
	if b.MaxToolCalls > 0 && b.ToolCallsUsed+1 > b.MaxToolCalls {
		return false
	}
	b.ToolCallsUsed++
	return true
}

func (b *Budget) RecordError() bool {
	if b.MaxErrors > 0 && b.ErrorsUsed+1 > b.MaxErrors {
		return false
	}
	b.ErrorsUsed++
	return true
}

func (b Budget) Expired(now time.Time) bool {
	if b.MaxDuration <= 0 || b.StartedAt.IsZero() {
		return false
	}
	return now.Sub(b.StartedAt) > b.MaxDuration
}
