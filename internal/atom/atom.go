package atom

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
)

// Store is a content-addressable object store that stores raw bytes by SHA-256 hash.
// Objects are stored at <root>/<hash[:2]>/<hash[2:]> to avoid too many files in one directory.
type Store struct {
	root string
}

// NewStore creates a new Store rooted at the given workspace path.
// The store manages objects under <workspaceRoot>/.prismagent/objects/.
func NewStore(workspaceRoot string) *Store {
	return &Store{
		root: filepath.Join(workspaceRoot, ".prismagent", "objects"),
	}
}

// Put stores data in the atom store and returns its SHA-256 hash.
// If the data already exists, the write is skipped (idempotent).
// Uses atomic write (temp file + rename) to prevent partial writes.
func (s *Store) Put(data []byte) (string, error) {
	hash := sha256.Sum256(data)
	hashStr := hex.EncodeToString(hash[:])

	path := s.Path(hashStr)
	dir := filepath.Dir(path)

	// If the file already exists, skip write
	if _, err := os.Stat(path); err == nil {
		return hashStr, nil
	}

	// Create directory if needed
	if err := os.MkdirAll(dir, 0755); err != nil {
		return "", fmt.Errorf("atom: mkdir %s: %w", dir, err)
	}

	// Atomic write: write to temp file, then rename
	tmp, err := os.CreateTemp(dir, ".tmp-*")
	if err != nil {
		return "", fmt.Errorf("atom: create temp file: %w", err)
	}
	tmpPath := tmp.Name()

	// Clean up temp file on error
	defer func() {
		if err != nil {
			os.Remove(tmpPath)
		}
	}()

	if _, err = tmp.Write(data); err != nil {
		tmp.Close()
		return "", fmt.Errorf("atom: write temp file: %w", err)
	}

	if err = tmp.Close(); err != nil {
		return "", fmt.Errorf("atom: close temp file: %w", err)
	}

	if err = os.Rename(tmpPath, path); err != nil {
		return "", fmt.Errorf("atom: rename %s -> %s: %w", tmpPath, path, err)
	}

	return hashStr, nil
}

// Get retrieves the data for the given hash from the atom store.
// Returns an error if the atom is not found.
func (s *Store) Get(hash string) ([]byte, error) {
	path := s.Path(hash)
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, fmt.Errorf("atom: not found: %s", hash)
		}
		return nil, fmt.Errorf("atom: read %s: %w", path, err)
	}
	return data, nil
}

// Has returns true if an atom with the given hash exists in the store.
func (s *Store) Has(hash string) bool {
	_, err := os.Stat(s.Path(hash))
	return err == nil
}

// Path returns the full filesystem path for an atom with the given hash.
// The path follows the layout: <root>/<hash[:2]>/<hash[2:]>
func (s *Store) Path(hash string) string {
	return filepath.Join(s.root, hash[:2], hash[2:])
}
