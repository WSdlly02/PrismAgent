package contextprovider

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type Bundle struct {
	Text  string
	Files []string
}

type Provider interface {
	Collect(ctx context.Context, query string) (Bundle, error)
}

type LocalProvider struct {
	Root          string
	MaxFileBytes  int
	MaxTotalBytes int
}

func NewLocalProvider(root string) LocalProvider {
	return LocalProvider{
		Root:          root,
		MaxFileBytes:  12_000,
		MaxTotalBytes: 40_000,
	}
}

func (p LocalProvider) Collect(_ context.Context, _ string) (Bundle, error) {
	candidates, err := p.candidateFiles()
	if err != nil {
		return Bundle{}, err
	}

	var builder strings.Builder
	files := make([]string, 0, len(candidates))
	total := 0
	for _, path := range candidates {
		data, err := os.ReadFile(path)
		if err != nil {
			return Bundle{}, err
		}
		if len(data) > p.MaxFileBytes {
			data = data[:p.MaxFileBytes]
		}
		if total+len(data) > p.MaxTotalBytes {
			break
		}
		rel, err := filepath.Rel(p.Root, path)
		if err != nil {
			return Bundle{}, err
		}
		builder.WriteString(fmt.Sprintf("\n\n--- %s ---\n", rel))
		builder.Write(data)
		files = append(files, rel)
		total += len(data)
	}
	return Bundle{
		Text:  strings.TrimSpace(builder.String()),
		Files: files,
	}, nil
}

func (p LocalProvider) candidateFiles() ([]string, error) {
	priority := []string{
		"ARCHTECTURE.md",
		"ARCHITECTURE.md",
		"README.md",
		"go.mod",
	}
	files := make([]string, 0)
	seen := make(map[string]bool)
	for _, name := range priority {
		path := filepath.Join(p.Root, name)
		if info, err := os.Stat(path); err == nil && !info.IsDir() {
			files = append(files, path)
			seen[path] = true
		}
	}
	if err := filepath.WalkDir(p.Root, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			switch entry.Name() {
			case ".git", ".prismagent":
				return filepath.SkipDir
			}
			return nil
		}
		if seen[path] {
			return nil
		}
		name := entry.Name()
		if strings.HasPrefix(name, "discussion") && strings.HasSuffix(name, ".md") {
			files = append(files, path)
			seen[path] = true
		}
		return nil
	}); err != nil {
		return nil, err
	}
	sort.Strings(files)
	return files, nil
}
