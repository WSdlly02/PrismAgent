package unit

import (
	"testing"

	"prismagent/internal/core"
)

func TestChainSaveAndLoad(t *testing.T) {
	tmp := t.TempDir()
	runID := "test-run-chain"
	agentID := "agent-1"

	chain := core.AgentChain{
		AgentID:  agentID,
		Chain:    []string{"unit-a", "unit-b", "unit-c"},
		Head:     "unit-c",
		Children: []string{},
	}

	if err := SaveChain(tmp, runID, chain); err != nil {
		t.Fatalf("SaveChain failed: %v", err)
	}

	got, err := LoadChain(tmp, runID, agentID)
	if err != nil {
		t.Fatalf("LoadChain failed: %v", err)
	}

	if got.AgentID != agentID {
		t.Fatalf("AgentID mismatch: got %s, want %s", got.AgentID, agentID)
	}
	if len(got.Chain) != 3 {
		t.Fatalf("Chain length mismatch: got %d, want 3", len(got.Chain))
	}
	if got.Head != "unit-c" {
		t.Fatalf("Head mismatch: got %s, want unit-c", got.Head)
	}
}

func TestChainLoadNonexistent(t *testing.T) {
	tmp := t.TempDir()

	// Loading a chain that doesn't exist should return an empty chain, not an error.
	chain, err := LoadChain(tmp, "no-run", "no-agent")
	if err != nil {
		t.Fatalf("LoadChain failed for nonexistent chain: %v", err)
	}
	if chain.AgentID != "no-agent" {
		t.Fatalf("AgentID mismatch: got %s, want no-agent", chain.AgentID)
	}
	if len(chain.Chain) != 0 {
		t.Fatalf("expected empty chain, got %d elements", len(chain.Chain))
	}
}

func TestChainAppend(t *testing.T) {
	tmp := t.TempDir()
	runID := "test-run-append"
	agentID := "agent-append"

	// Start with a chain that has two entries.
	chain := core.AgentChain{
		AgentID:  agentID,
		Chain:    []string{"unit-1", "unit-2"},
		Head:     "unit-2",
		Children: []string{},
	}
	if err := SaveChain(tmp, runID, chain); err != nil {
		t.Fatalf("SaveChain failed: %v", err)
	}

	// Append a third entry.
	if err := AppendToChain(tmp, runID, agentID, "unit-3"); err != nil {
		t.Fatalf("AppendToChain failed: %v", err)
	}

	// Load and verify.
	got, err := LoadChain(tmp, runID, agentID)
	if err != nil {
		t.Fatalf("LoadChain failed: %v", err)
	}

	if len(got.Chain) != 3 {
		t.Fatalf("Chain length mismatch: got %d, want 3", len(got.Chain))
	}
	if got.Chain[2] != "unit-3" {
		t.Fatalf("Chain[2] mismatch: got %s, want unit-3", got.Chain[2])
	}
	if got.Head != "unit-3" {
		t.Fatalf("Head mismatch: got %s, want unit-3", got.Head)
	}
}

func TestChainAppendToNew(t *testing.T) {
	tmp := t.TempDir()
	runID := "test-run-new-chain"
	agentID := "agent-new"

	// Append to a chain that doesn't exist yet.
	if err := AppendToChain(tmp, runID, agentID, "first-unit"); err != nil {
		t.Fatalf("AppendToChain failed: %v", err)
	}

	got, err := LoadChain(tmp, runID, agentID)
	if err != nil {
		t.Fatalf("LoadChain failed: %v", err)
	}

	if len(got.Chain) != 1 {
		t.Fatalf("Chain length mismatch: got %d, want 1", len(got.Chain))
	}
	if got.Head != "first-unit" {
		t.Fatalf("Head mismatch: got %s, want first-unit", got.Head)
	}
}
