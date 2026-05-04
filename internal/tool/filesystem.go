package tool

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
)

const (
	ToolListFiles           = "list_files"
	ToolReadFile            = "read_file"
	ToolReplaceInFile       = "replace_in_file"
	ToolWriteFileCreateOnly = "write_file_create_only"
)

type FileSystemTools struct {
	root string
}

func NewFileSystemTools(root string) []Tool {
	fs := FileSystemTools{root: root}
	return []Tool{
		listFilesTool{fs: fs},
		readFileTool{fs: fs},
		replaceInFileTool{fs: fs},
		writeFileCreateOnlyTool{fs: fs},
	}
}

type listFilesTool struct{ fs FileSystemTools }

func (t listFilesTool) Name() string { return ToolListFiles }

func (t listFilesTool) Definition() Definition {
	return Definition{
		Name:        ToolListFiles,
		Description: "List files in the workspace. Use this before reading files when you need to inspect project structure.",
		Parameters: objectSchema(map[string]any{
			"path": map[string]any{
				"type":        "string",
				"description": "Workspace-relative path to list. Use . for workspace root.",
			},
			"recursive": map[string]any{
				"type":        "string",
				"description": "Whether to list recursively: true or false.",
			},
			"max_entries": map[string]any{
				"type":        "string",
				"description": "Maximum entries to return, as a decimal string.",
			},
		}, []string{"path", "recursive", "max_entries"}),
	}
}

func (t listFilesTool) Call(_ context.Context, req Request) (Result, error) {
	path, err := t.fs.resolve(req.Args["path"])
	if err != nil {
		return Result{}, err
	}
	recursive := parseBool(req.Args["recursive"])
	maxEntries := parseInt(req.Args["max_entries"], 200)
	entries := make([]string, 0)

	if recursive {
		err = filepath.WalkDir(path, func(current string, entry os.DirEntry, walkErr error) error {
			if walkErr != nil {
				return walkErr
			}
			if t.fs.isBlocked(current) {
				if entry.IsDir() {
					return filepath.SkipDir
				}
				return nil
			}
			if current == path {
				return nil
			}
			rel, err := filepath.Rel(t.fs.root, current)
			if err != nil {
				return err
			}
			if entry.IsDir() {
				rel += "/"
			}
			entries = append(entries, rel)
			if len(entries) >= maxEntries {
				return errStopWalk
			}
			return nil
		})
		if err != nil && err != errStopWalk {
			return Result{}, err
		}
	} else {
		children, err := os.ReadDir(path)
		if err != nil {
			return Result{}, err
		}
		for _, child := range children {
			current := filepath.Join(path, child.Name())
			if t.fs.isBlocked(current) {
				continue
			}
			rel, err := filepath.Rel(t.fs.root, current)
			if err != nil {
				return Result{}, err
			}
			if child.IsDir() {
				rel += "/"
			}
			entries = append(entries, rel)
			if len(entries) >= maxEntries {
				break
			}
		}
	}
	sort.Strings(entries)
	payload, err := json.MarshalIndent(map[string]any{
		"entries":   entries,
		"truncated": len(entries) >= maxEntries,
	}, "", "  ")
	if err != nil {
		return Result{}, err
	}
	return Result{
		RawOutput: string(payload),
		Summary:   fmt.Sprintf("listed %d entries", len(entries)),
		Metadata: map[string]string{
			"entries": strconv.Itoa(len(entries)),
		},
	}, nil
}

type readFileTool struct{ fs FileSystemTools }

func (t readFileTool) Name() string { return ToolReadFile }

func (t readFileTool) Definition() Definition {
	return Definition{
		Name:        ToolReadFile,
		Description: "Read a text file from the workspace.",
		Parameters: objectSchema(map[string]any{
			"path": map[string]any{
				"type":        "string",
				"description": "Workspace-relative file path to read.",
			},
			"limit": map[string]any{
				"type":        "string",
				"description": "Maximum bytes to read, as a decimal string.",
			},
		}, []string{"path", "limit"}),
	}
}

func (t readFileTool) Call(_ context.Context, req Request) (Result, error) {
	path, err := t.fs.resolve(req.Args["path"])
	if err != nil {
		return Result{}, err
	}
	if t.fs.isBlocked(path) {
		return Result{}, fmt.Errorf("path is blocked: %s", req.Args["path"])
	}
	limit := parseInt(req.Args["limit"], 12000)
	if limit <= 0 {
		limit = 12000
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return Result{}, err
	}
	truncated := false
	if len(data) > limit {
		data = data[:limit]
		truncated = true
	}
	return Result{
		RawOutput: string(data),
		Summary:   fmt.Sprintf("read %d bytes", len(data)),
		Metadata: map[string]string{
			"truncated": strconv.FormatBool(truncated),
			"bytes":     strconv.Itoa(len(data)),
		},
	}, nil
}

type replaceInFileTool struct{ fs FileSystemTools }

func (t replaceInFileTool) Name() string { return ToolReplaceInFile }

func (t replaceInFileTool) Definition() Definition {
	return Definition{
		Name:        ToolReplaceInFile,
		Description: "Replace a unique text block in a workspace file. The old text must match exactly once.",
		Parameters: objectSchema(map[string]any{
			"path": map[string]any{
				"type":        "string",
				"description": "Workspace-relative file path to modify.",
			},
			"old": map[string]any{
				"type":        "string",
				"description": "Exact old text to replace. Must occur exactly once.",
			},
			"new": map[string]any{
				"type":        "string",
				"description": "Replacement text.",
			},
		}, []string{"path", "old", "new"}),
	}
}

func (t replaceInFileTool) Call(_ context.Context, req Request) (Result, error) {
	path, err := t.fs.resolve(req.Args["path"])
	if err != nil {
		return Result{}, err
	}
	if t.fs.isBlocked(path) {
		return Result{}, fmt.Errorf("path is blocked: %s", req.Args["path"])
	}
	old := req.Args["old"]
	newText := req.Args["new"]
	if old == "" {
		return Result{}, fmt.Errorf("old text is required")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return Result{}, err
	}
	content := string(data)
	count := strings.Count(content, old)
	if count == 0 {
		return Result{}, fmt.Errorf("old text not found")
	}
	if count > 1 {
		return Result{}, fmt.Errorf("old text is not unique: %d matches", count)
	}
	updated := strings.Replace(content, old, newText, 1)
	if err := os.WriteFile(path, []byte(updated), 0o644); err != nil {
		return Result{}, err
	}
	return Result{
		RawOutput: "replacement applied",
		Summary:   "replacement applied",
		Metadata: map[string]string{
			"old_sha256": sha256Hex([]byte(content)),
			"new_sha256": sha256Hex([]byte(updated)),
		},
	}, nil
}

type writeFileCreateOnlyTool struct{ fs FileSystemTools }

func (t writeFileCreateOnlyTool) Name() string { return ToolWriteFileCreateOnly }

func (t writeFileCreateOnlyTool) Definition() Definition {
	return Definition{
		Name:        ToolWriteFileCreateOnly,
		Description: "Create a new workspace file. Fails if the file already exists.",
		Parameters: objectSchema(map[string]any{
			"path": map[string]any{
				"type":        "string",
				"description": "Workspace-relative file path to create.",
			},
			"content": map[string]any{
				"type":        "string",
				"description": "File content.",
			},
		}, []string{"path", "content"}),
	}
}

func (t writeFileCreateOnlyTool) Call(_ context.Context, req Request) (Result, error) {
	path, err := t.fs.resolve(req.Args["path"])
	if err != nil {
		return Result{}, err
	}
	if t.fs.isBlocked(path) {
		return Result{}, fmt.Errorf("path is blocked: %s", req.Args["path"])
	}
	if _, err := os.Stat(path); err == nil {
		return Result{}, fmt.Errorf("file already exists: %s", req.Args["path"])
	} else if !os.IsNotExist(err) {
		return Result{}, err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return Result{}, err
	}
	content := req.Args["content"]
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		return Result{}, err
	}
	return Result{
		RawOutput: "file created",
		Summary:   "file created",
		Metadata: map[string]string{
			"sha256": sha256Hex([]byte(content)),
		},
	}, nil
}

func (fs FileSystemTools) resolve(path string) (string, error) {
	if strings.TrimSpace(path) == "" {
		path = "."
	}
	cleaned := filepath.Clean(path)
	if filepath.IsAbs(cleaned) {
		return "", fmt.Errorf("absolute paths are not allowed")
	}
	full := filepath.Join(fs.root, cleaned)
	rel, err := filepath.Rel(fs.root, full)
	if err != nil {
		return "", err
	}
	if rel == ".." || strings.HasPrefix(rel, ".."+string(os.PathSeparator)) {
		return "", fmt.Errorf("path escapes workspace: %s", path)
	}
	return full, nil
}

func (fs FileSystemTools) isBlocked(path string) bool {
	rel, err := filepath.Rel(fs.root, path)
	if err != nil {
		return true
	}
	for _, part := range strings.Split(rel, string(os.PathSeparator)) {
		if part == ".git" || part == ".prismagent" {
			return true
		}
	}
	return false
}

func parseBool(value string) bool {
	value = strings.ToLower(strings.TrimSpace(value))
	return value == "true" || value == "1" || value == "yes"
}

func parseInt(value string, fallback int) int {
	if strings.TrimSpace(value) == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil {
		return fallback
	}
	return parsed
}

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

var errStopWalk = fmt.Errorf("stop walk")

func objectSchema(properties map[string]any, required []string) map[string]any {
	return map[string]any{
		"type":                 "object",
		"properties":           properties,
		"required":             required,
		"additionalProperties": false,
	}
}
