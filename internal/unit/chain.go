package unit

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"prismagent/internal/core"
)

// chainPath returns the filesystem path for an agent chain within a run.
func chainPath(workspaceRoot, runID, agentID string) string {
	return filepath.Join(workspaceRoot, ".prismagent", "runs", runID, "agent-"+agentID+".json")
}

// LoadChain reads an AgentChain from disk.
func LoadChain(workspaceRoot string, runID string, agentID string) (core.AgentChain, error) {
	path := chainPath(workspaceRoot, runID, agentID)

	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return core.AgentChain{
				AgentID:  agentID,
				Chain:    []string{},
				Head:     "",
				Children: []string{},
			}, nil
		}
		return core.AgentChain{}, fmt.Errorf("chain: read %s: %w", path, err)
	}

	var chain core.AgentChain
	if err := json.Unmarshal(data, &chain); err != nil {
		return core.AgentChain{}, fmt.Errorf("chain: unmarshal %s: %w", agentID, err)
	}

	return chain, nil
}

// SaveChain writes an AgentChain to disk.
func SaveChain(workspaceRoot string, runID string, chain core.AgentChain) error {
	path := chainPath(workspaceRoot, runID, chain.AgentID)
	dir := filepath.Dir(path)

	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("chain: mkdir %s: %w", dir, err)
	}

	data, err := json.MarshalIndent(chain, "", "  ")
	if err != nil {
		return fmt.Errorf("chain: marshal %s: %w", chain.AgentID, err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("chain: write %s: %w", path, err)
	}

	return nil
}

// AppendToChain loads a chain, appends a unit UUID, updates Head, and saves.
func AppendToChain(workspaceRoot string, runID string, agentID string, unitUUID string) error {
	chain, err := LoadChain(workspaceRoot, runID, agentID)
	if err != nil {
		return err
	}

	chain.Chain = append(chain.Chain, unitUUID)
	chain.Head = unitUUID

	return SaveChain(workspaceRoot, runID, chain)
}
