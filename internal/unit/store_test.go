package unit

import (
	"testing"
	"time"

	"prismagent/internal/core"
)

func makeTestUnit(uuid string) core.Unit {
	return core.Unit{
		UUID:       uuid,
		AtomHash:   "abc123def456abc123def456abc123def456abc123def456abc123def456abc1",
		Kind:       core.UnitMessage,
		Role:       core.RoleUser,
		Scope:      core.ScopeRun,
		Visibility: core.VisibilityUser,
		Metadata:   map[string]string{"key": "value"},
		CreatedAt:  time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC),
	}
}

func TestStorePutAndGet(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)
	runID := "test-run-001"

	u := makeTestUnit("unit-aaa-111")
	if err := store.Put(runID, u); err != nil {
		t.Fatalf("Put failed: %v", err)
	}

	got, err := store.Get(runID, "unit-aaa-111")
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}

	if got.UUID != u.UUID {
		t.Fatalf("UUID mismatch: got %s, want %s", got.UUID, u.UUID)
	}
	if got.Kind != u.Kind {
		t.Fatalf("Kind mismatch: got %s, want %s", got.Kind, u.Kind)
	}
	if got.Role != u.Role {
		t.Fatalf("Role mismatch: got %s, want %s", got.Role, u.Role)
	}
	if got.AtomHash != u.AtomHash {
		t.Fatalf("AtomHash mismatch: got %s, want %s", got.AtomHash, u.AtomHash)
	}
	if got.Metadata["key"] != "value" {
		t.Fatalf("Metadata mismatch: got %v, want key=value", got.Metadata)
	}
}

func TestStoreGetNotFound(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)

	_, err := store.Get("nonexistent-run", "nonexistent-unit")
	if err == nil {
		t.Fatal("expected error for non-existent unit, got nil")
	}
}

func TestStoreList(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)
	runID := "test-run-list"

	units := []core.Unit{
		makeTestUnit("unit-1"),
		makeTestUnit("unit-2"),
		makeTestUnit("unit-3"),
	}

	for _, u := range units {
		if err := store.Put(runID, u); err != nil {
			t.Fatalf("Put failed: %v", err)
		}
	}

	got, err := store.List(runID, []string{"unit-1", "unit-2", "unit-3"})
	if err != nil {
		t.Fatalf("List failed: %v", err)
	}

	if len(got) != 3 {
		t.Fatalf("List returned %d units, want 3", len(got))
	}

	for i, u := range got {
		if u.UUID != units[i].UUID {
			t.Fatalf("unit[%d] UUID mismatch: got %s, want %s", i, u.UUID, units[i].UUID)
		}
	}
}

func TestStoreListPartialNotFound(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)
	runID := "test-run-partial"

	u := makeTestUnit("exists")
	if err := store.Put(runID, u); err != nil {
		t.Fatalf("Put failed: %v", err)
	}

	_, err := store.List(runID, []string{"exists", "does-not-exist"})
	if err == nil {
		t.Fatal("expected error when one unit is missing, got nil")
	}
}
