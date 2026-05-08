package unit

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sync"

	"prismagent/internal/core"
)

// Store manages persistence of Units on disk.
// Units are stored as JSON files at .prismagent/runs/<run_id>/units/<uuid>.json.
type Store struct {
	workspaceRoot string
}

// NewStore creates a new Unit Store rooted at the given workspace path.
func NewStore(workspaceRoot string) *Store {
	return &Store{workspaceRoot: workspaceRoot}
}

// unitPath returns the filesystem path for a unit within a run.
func (s *Store) unitPath(runID, uuid string) string {
	return filepath.Join(s.workspaceRoot, ".prismagent", "runs", runID, "units", uuid+".json")
}

// Put serializes a Unit to JSON and writes it to disk.
func (s *Store) Put(runID string, u core.Unit) error {
	path := s.unitPath(runID, u.UUID)
	dir := filepath.Dir(path)

	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("unit: mkdir %s: %w", dir, err)
	}

	data, err := json.MarshalIndent(u, "", "  ")
	if err != nil {
		return fmt.Errorf("unit: marshal %s: %w", u.UUID, err)
	}

	if err := os.WriteFile(path, data, 0644); err != nil {
		return fmt.Errorf("unit: write %s: %w", path, err)
	}

	return nil
}

// Get reads and deserializes a Unit from disk by run ID and UUID.
func (s *Store) Get(runID string, uuid string) (core.Unit, error) {
	path := s.unitPath(runID, uuid)

	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return core.Unit{}, fmt.Errorf("unit: not found: %s", uuid)
		}
		return core.Unit{}, fmt.Errorf("unit: read %s: %w", path, err)
	}

	var u core.Unit
	if err := json.Unmarshal(data, &u); err != nil {
		return core.Unit{}, fmt.Errorf("unit: unmarshal %s: %w", uuid, err)
	}

	return u, nil
}

// List loads multiple Units by UUID concurrently.
func (s *Store) List(runID string, uuids []string) ([]core.Unit, error) {
	results := make([]core.Unit, len(uuids))
	errs := make([]error, len(uuids))

	var wg sync.WaitGroup
	for i, uuid := range uuids {
		wg.Add(1)
		go func(idx int, id string) {
			defer wg.Done()
			results[idx], errs[idx] = s.Get(runID, id)
		}(i, uuid)
	}
	wg.Wait()

	for _, err := range errs {
		if err != nil {
			return nil, err
		}
	}

	return results, nil
}
