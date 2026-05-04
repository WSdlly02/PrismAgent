package core

import (
	"testing"
	"time"
)

func TestBudgetConsumesWithinLimits(t *testing.T) {
	budget := NewBudget(10, 1, 1, time.Minute)

	if !budget.ConsumeTokens(10) {
		t.Fatal("expected token consumption to fit budget")
	}
	if budget.ConsumeTokens(1) {
		t.Fatal("expected token consumption to exceed budget")
	}
	if !budget.ConsumeToolCall() {
		t.Fatal("expected first tool call to fit budget")
	}
	if budget.ConsumeToolCall() {
		t.Fatal("expected second tool call to exceed budget")
	}
}
