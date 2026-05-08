package atom

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestPutAndGet(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)

	data := []byte("hello, world")
	hash, err := store.Put(data)
	if err != nil {
		t.Fatalf("Put failed: %v", err)
	}

	// Verify hash is correct
	expected := sha256.Sum256(data)
	expectedHash := hex.EncodeToString(expected[:])
	if hash != expectedHash {
		t.Fatalf("hash mismatch: got %s, want %s", hash, expectedHash)
	}

	// Get and verify content
	got, err := store.Get(hash)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if string(got) != string(data) {
		t.Fatalf("content mismatch: got %q, want %q", got, data)
	}
}

func TestPutIdempotent(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)

	data := []byte("idempotent data")

	hash1, err := store.Put(data)
	if err != nil {
		t.Fatalf("first Put failed: %v", err)
	}

	hash2, err := store.Put(data)
	if err != nil {
		t.Fatalf("second Put failed: %v", err)
	}

	if hash1 != hash2 {
		t.Fatalf("hashes differ: %s != %s", hash1, hash2)
	}

	// Verify data is still correct
	got, err := store.Get(hash1)
	if err != nil {
		t.Fatalf("Get failed: %v", err)
	}
	if string(got) != string(data) {
		t.Fatalf("content mismatch: got %q, want %q", got, data)
	}
}

func TestGetNotFound(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)

	_, err := store.Get("0000000000000000000000000000000000000000000000000000000000000000")
	if err == nil {
		t.Fatal("expected error for non-existent hash, got nil")
	}
	if !strings.Contains(err.Error(), "not found") {
		t.Fatalf("expected 'not found' error, got: %v", err)
	}
}

func TestHas(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)

	data := []byte("test has data")
	hash, err := store.Put(data)
	if err != nil {
		t.Fatalf("Put failed: %v", err)
	}

	if !store.Has(hash) {
		t.Fatal("Has returned false for existing atom")
	}

	// Non-existent hash
	bogusHash := "0000000000000000000000000000000000000000000000000000000000000000"
	if store.Has(bogusHash) {
		t.Fatal("Has returned true for non-existent atom")
	}
}

func TestPathFormat(t *testing.T) {
	tmp := t.TempDir()
	store := NewStore(tmp)

	data := []byte("path format test")
	hash, err := store.Put(data)
	if err != nil {
		t.Fatalf("Put failed: %v", err)
	}

	path := store.Path(hash)

	// Path should be <root>/<hash[:2]>/<hash[2:]>
	expectedPath := filepath.Join(tmp, ".prismagent", "objects", hash[:2], hash[2:])
	if path != expectedPath {
		t.Fatalf("path mismatch:\n  got:  %s\n  want: %s", path, expectedPath)
	}

	// Verify the file actually exists at that path
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("file does not exist at expected path: %v", err)
	}

	// Verify directory is exactly 2 chars
	dir := filepath.Dir(path)
	dirName := filepath.Base(dir)
	if len(dirName) != 2 {
		t.Fatalf("directory name length is %d, want 2: %s", len(dirName), dirName)
	}
}
