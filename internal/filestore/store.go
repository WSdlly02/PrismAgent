package filestore

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"prismagent/internal/core"
)

const stateDirName = ".prismagent"

type Store struct {
	mu   sync.Mutex
	root string
}

func New(root string) *Store {
	return &Store{root: root}
}

func (s *Store) InitWorkspace(_ context.Context, workspace core.Workspace) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if workspace.Root == "" {
		workspace.Root = s.root
	}
	if err := os.MkdirAll(filepath.Join(s.stateRoot(), "context"), 0o755); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(s.stateRoot(), "runs"), 0o755); err != nil {
		return err
	}
	if err := writeJSON(filepath.Join(s.stateRoot(), "workspace.json"), workspace); err != nil {
		return err
	}
	configPath := filepath.Join(s.stateRoot(), "config.toml")
	if _, err := os.Stat(configPath); errors.Is(err, os.ErrNotExist) {
		return os.WriteFile(configPath, []byte("# Prism Agent workspace configuration\n"), 0o644)
	}
	return nil
}

func (s *Store) GetWorkspace(_ context.Context) (core.Workspace, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	var workspace core.Workspace
	if err := readJSON(filepath.Join(s.stateRoot(), "workspace.json"), &workspace); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return core.Workspace{}, fmt.Errorf("workspace not found")
		}
		return core.Workspace{}, err
	}
	return workspace, nil
}

func (s *Store) CreateRun(_ context.Context, run core.Run) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.ensureRunDirs(run.ID); err != nil {
		return err
	}
	return writeJSON(s.runFile(run.ID, "run.json"), run)
}

func (s *Store) GetRun(_ context.Context, id core.RunID) (core.Run, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	var run core.Run
	if err := readJSON(s.runFile(id, "run.json"), &run); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return core.Run{}, fmt.Errorf("run not found: %s", id)
		}
		return core.Run{}, err
	}
	return run, nil
}

func (s *Store) UpdateRun(_ context.Context, run core.Run) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if _, err := os.Stat(s.runFile(run.ID, "run.json")); errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("run not found: %s", run.ID)
	} else if err != nil {
		return err
	}
	return writeJSON(s.runFile(run.ID, "run.json"), run)
}

func (s *Store) ListRuns(_ context.Context) ([]core.Run, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	runIDs, err := s.listRunIDs()
	if err != nil {
		return nil, err
	}
	runs := make([]core.Run, 0, len(runIDs))
	for _, runID := range runIDs {
		var run core.Run
		if err := readJSON(s.runFile(runID, "run.json"), &run); err != nil {
			return nil, err
		}
		runs = append(runs, run)
	}
	return runs, nil
}

func (s *Store) SetCurrentRun(_ context.Context, id core.RunID) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if _, err := os.Stat(s.runFile(id, "run.json")); errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("run not found: %s", id)
	} else if err != nil {
		return err
	}

	linkPath := s.currentRunPath()
	if err := os.Remove(linkPath); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if err := os.Symlink(id.String(), linkPath); err == nil {
		return nil
	}
	return writeFileAtomic(linkPath, []byte(id.String()+"\n"))
}

func (s *Store) GetCurrentRun(_ context.Context) (core.RunID, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	target, err := os.Readlink(s.currentRunPath())
	if err == nil {
		return core.RunID(filepath.Base(target)), nil
	}
	if !errors.Is(err, os.ErrInvalid) && !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	data, readErr := os.ReadFile(s.currentRunPath())
	if errors.Is(readErr, os.ErrNotExist) {
		return "", fmt.Errorf("current run is not set")
	}
	if readErr != nil {
		return "", readErr
	}
	id := strings.TrimSpace(string(data))
	if id == "" {
		return "", fmt.Errorf("current run is empty")
	}
	return core.RunID(id), nil
}

func (s *Store) WriteAgents(_ context.Context, runID core.RunID, agents []core.Agent) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.ensureRunDirs(runID); err != nil {
		return err
	}
	return writeJSON(s.runFile(runID, "agents.json"), agents)
}

func (s *Store) ListAgents(_ context.Context, runID core.RunID) ([]core.Agent, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	var agents []core.Agent
	if err := readJSON(s.runFile(runID, "agents.json"), &agents); errors.Is(err, os.ErrNotExist) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	return agents, nil
}

func (s *Store) AppendConversationTurn(_ context.Context, turn core.ConversationTurn) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.ensureRunDirs(turn.RunID); err != nil {
		return err
	}
	file, err := os.OpenFile(s.runFile(turn.RunID, "conversation.jsonl"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return err
	}
	defer file.Close()

	encoded, err := json.Marshal(turn)
	if err != nil {
		return err
	}
	if _, err := file.Write(append(encoded, '\n')); err != nil {
		return err
	}
	return nil
}

func (s *Store) ListConversationTurns(_ context.Context, runID core.RunID) ([]core.ConversationTurn, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	file, err := os.Open(s.runFile(runID, "conversation.jsonl"))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	defer file.Close()

	turns := make([]core.ConversationTurn, 0)
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		var turn core.ConversationTurn
		if err := json.Unmarshal(scanner.Bytes(), &turn); err != nil {
			return nil, err
		}
		turns = append(turns, turn)
	}
	return turns, scanner.Err()
}

func (s *Store) WriteRunArtifact(_ context.Context, runID core.RunID, name string, body string) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.ensureRunDirs(runID); err != nil {
		return err
	}
	return writeFileAtomic(s.runFile(runID, name), []byte(body))
}

func (s *Store) ReadRunArtifact(_ context.Context, runID core.RunID, name string) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	data, err := os.ReadFile(s.runFile(runID, name))
	if errors.Is(err, os.ErrNotExist) {
		return "", fmt.Errorf("run artifact not found: %s/%s", runID, name)
	}
	if err != nil {
		return "", err
	}
	return string(data), nil
}

func (s *Store) CreateTask(_ context.Context, task core.Task) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	tasks, err := s.readTasks(task.RunID)
	if err != nil {
		return err
	}
	for _, existing := range tasks {
		if existing.ID == task.ID {
			return fmt.Errorf("task already exists: %s", task.ID)
		}
	}
	tasks = append(tasks, task)
	return s.writeTasks(task.RunID, tasks)
}

func (s *Store) GetTask(_ context.Context, id core.TaskID) (core.Task, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	runs, err := s.listRunIDs()
	if err != nil {
		return core.Task{}, err
	}
	for _, runID := range runs {
		tasks, err := s.readTasks(runID)
		if err != nil {
			return core.Task{}, err
		}
		for _, task := range tasks {
			if task.ID == id {
				return task, nil
			}
		}
	}
	return core.Task{}, fmt.Errorf("task not found: %s", id)
}

func (s *Store) UpdateTask(_ context.Context, task core.Task) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	tasks, err := s.readTasks(task.RunID)
	if err != nil {
		return err
	}
	for i := range tasks {
		if tasks[i].ID == task.ID {
			tasks[i] = task
			return s.writeTasks(task.RunID, tasks)
		}
	}
	return fmt.Errorf("task not found: %s", task.ID)
}

func (s *Store) ListTasks(_ context.Context, runID core.RunID) ([]core.Task, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	return s.readTasks(runID)
}

func (s *Store) PutContextObject(_ context.Context, object core.ContextObject) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	objects, err := s.readContextObjects(object.Scope)
	if err != nil {
		return err
	}
	replaced := false
	for i := range objects {
		if objects[i].ID == object.ID {
			objects[i] = object
			replaced = true
			break
		}
	}
	if !replaced {
		objects = append(objects, object)
	}
	return s.writeContextObjects(object.Scope, objects)
}

func (s *Store) GetContextObject(_ context.Context, id core.ContextObjectID) (core.ContextObject, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	objects, err := s.readAllContextObjects()
	if err != nil {
		return core.ContextObject{}, err
	}
	for _, object := range objects {
		if object.ID == id {
			return object, nil
		}
	}
	return core.ContextObject{}, fmt.Errorf("context object not found: %s", id)
}

func (s *Store) ListContextObjects(_ context.Context, scope core.ContextScope) ([]core.ContextObject, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	objects, err := s.readAllContextObjects()
	if err != nil {
		return nil, err
	}
	filtered := make([]core.ContextObject, 0, len(objects))
	for _, object := range objects {
		if scopeMatches(object.Scope, scope) {
			filtered = append(filtered, object)
		}
	}
	return filtered, nil
}

func (s *Store) CreateSnapshot(_ context.Context, snapshot core.Snapshot, state core.SnapshotState) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.ensureRunDirs(snapshot.RunID); err != nil {
		return err
	}
	path := s.snapshotPath(snapshot.RunID, snapshot.ID)
	if _, err := os.Stat(path); err == nil {
		return fmt.Errorf("snapshot already exists: %s", snapshot.ID)
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	return writeJSON(path, core.SnapshotRecord{
		Snapshot: snapshot,
		State:    state,
	})
}

func (s *Store) GetSnapshot(_ context.Context, id core.SnapshotID) (core.SnapshotRecord, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	runs, err := s.listRunIDs()
	if err != nil {
		return core.SnapshotRecord{}, err
	}
	for _, runID := range runs {
		path := s.snapshotPath(runID, id)
		if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
			continue
		} else if err != nil {
			return core.SnapshotRecord{}, err
		}
		var record core.SnapshotRecord
		if err := readJSON(path, &record); err != nil {
			return core.SnapshotRecord{}, err
		}
		return record, nil
	}
	return core.SnapshotRecord{}, fmt.Errorf("snapshot not found: %s", id)
}

func (s *Store) RestoreSnapshot(ctx context.Context, id core.SnapshotID) (core.SnapshotState, error) {
	record, err := s.GetSnapshot(ctx, id)
	if err != nil {
		return core.SnapshotState{}, err
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.writeTasks(record.Snapshot.RunID, record.State.Tasks); err != nil {
		return core.SnapshotState{}, err
	}
	if err := s.writeContextObjects(core.ContextScope{Type: core.ScopeRun, RunID: record.Snapshot.RunID}, record.State.ContextObjects); err != nil {
		return core.SnapshotState{}, err
	}
	return record.State, nil
}

func (s *Store) Emit(_ context.Context, event core.Event) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if err := s.ensureRunDirs(event.RunID); err != nil {
		return err
	}
	file, err := os.OpenFile(s.runFile(event.RunID, "events.jsonl"), os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return err
	}
	defer file.Close()

	encoded, err := json.Marshal(event)
	if err != nil {
		return err
	}
	if _, err := file.Write(append(encoded, '\n')); err != nil {
		return err
	}
	return nil
}

func (s *Store) ListEvents(_ context.Context, runID core.RunID) ([]core.Event, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	path := s.runFile(runID, "events.jsonl")
	file, err := os.Open(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	defer file.Close()

	events := make([]core.Event, 0)
	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		var event core.Event
		if err := json.Unmarshal(scanner.Bytes(), &event); err != nil {
			return nil, err
		}
		events = append(events, event)
	}
	return events, scanner.Err()
}

func (s *Store) stateRoot() string {
	return filepath.Join(s.root, stateDirName)
}

func (s *Store) ensureRunDirs(runID core.RunID) error {
	if err := os.MkdirAll(s.runDir(runID), 0o755); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(s.runDir(runID), "snapshots"), 0o755); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Join(s.runDir(runID), "artifacts"), 0o755); err != nil {
		return err
	}
	return os.MkdirAll(filepath.Join(s.runDir(runID), "conversations"), 0o755)
}

func (s *Store) runDir(runID core.RunID) string {
	return filepath.Join(s.stateRoot(), "runs", runID.String())
}

func (s *Store) currentRunPath() string {
	return filepath.Join(s.stateRoot(), "runs", "current-run")
}

func (s *Store) runFile(runID core.RunID, name string) string {
	return filepath.Join(s.runDir(runID), name)
}

func (s *Store) snapshotPath(runID core.RunID, snapshotID core.SnapshotID) string {
	return filepath.Join(s.runDir(runID), "snapshots", snapshotID.String()+".json")
}

func (s *Store) workspaceContextPath() string {
	return filepath.Join(s.stateRoot(), "context", "objects.jsonl")
}

func (s *Store) readTasks(runID core.RunID) ([]core.Task, error) {
	path := s.runFile(runID, "tasks.json")
	var tasks []core.Task
	if err := readJSON(path, &tasks); errors.Is(err, os.ErrNotExist) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	return tasks, nil
}

func (s *Store) writeTasks(runID core.RunID, tasks []core.Task) error {
	if err := s.ensureRunDirs(runID); err != nil {
		return err
	}
	sort.Slice(tasks, func(i, j int) bool {
		return tasks[i].ID < tasks[j].ID
	})
	return writeJSON(s.runFile(runID, "tasks.json"), tasks)
}

func (s *Store) readContextObjects(scope core.ContextScope) ([]core.ContextObject, error) {
	path := s.contextPath(scope)
	return readContextObjectsFile(path)
}

func (s *Store) readAllContextObjects() ([]core.ContextObject, error) {
	objects := make([]core.ContextObject, 0)
	workspaceObjects, err := readContextObjectsFile(s.workspaceContextPath())
	if err != nil {
		return nil, err
	}
	objects = append(objects, workspaceObjects...)

	runIDs, err := s.listRunIDs()
	if err != nil {
		return nil, err
	}
	for _, runID := range runIDs {
		runObjects, err := readContextObjectsFile(s.runFile(runID, "context.jsonl"))
		if err != nil {
			return nil, err
		}
		objects = append(objects, runObjects...)
	}
	return objects, nil
}

func (s *Store) writeContextObjects(scope core.ContextScope, objects []core.ContextObject) error {
	path := s.contextPath(scope)
	if scope.Type == core.ScopeWorkspace {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return err
		}
	} else if scope.RunID != "" {
		if err := s.ensureRunDirs(scope.RunID); err != nil {
			return err
		}
	}
	return writeJSONLines(path, objects)
}

func (s *Store) contextPath(scope core.ContextScope) string {
	if scope.Type == core.ScopeWorkspace {
		return s.workspaceContextPath()
	}
	return s.runFile(scope.RunID, "context.jsonl")
}

func (s *Store) listRunIDs() ([]core.RunID, error) {
	entries, err := os.ReadDir(filepath.Join(s.stateRoot(), "runs"))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	runIDs := make([]core.RunID, 0, len(entries))
	for _, entry := range entries {
		if entry.Name() == "current-run" {
			continue
		}
		if entry.IsDir() {
			runIDs = append(runIDs, core.RunID(entry.Name()))
		}
	}
	sort.Slice(runIDs, func(i, j int) bool {
		return runIDs[i] < runIDs[j]
	})
	return runIDs, nil
}

func scopeMatches(actual, filter core.ContextScope) bool {
	if filter.Type != "" && actual.Type != filter.Type {
		return false
	}
	if filter.WorkspaceID != "" && actual.WorkspaceID != filter.WorkspaceID {
		return false
	}
	if filter.RunID != "" && actual.RunID != filter.RunID {
		return false
	}
	if filter.TaskID != "" && actual.TaskID != filter.TaskID {
		return false
	}
	return true
}

func readJSON(path string, target any) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	return json.NewDecoder(file).Decode(target)
}

func writeJSON(path string, value any) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	return writeFileAtomic(path, data)
}

func readContextObjectsFile(path string) ([]core.ContextObject, error) {
	file, err := os.Open(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	defer file.Close()

	objects := make([]core.ContextObject, 0)
	reader := bufio.NewReader(file)
	for {
		line, err := reader.ReadBytes('\n')
		if len(line) > 0 {
			var object core.ContextObject
			if unmarshalErr := json.Unmarshal(line, &object); unmarshalErr != nil {
				return nil, unmarshalErr
			}
			objects = append(objects, object)
		}
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return nil, err
		}
	}
	return objects, nil
}

func writeJSONLines(path string, values []core.ContextObject) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	file, err := os.CreateTemp(filepath.Dir(path), ".tmp-")
	if err != nil {
		return err
	}
	tmpPath := file.Name()
	defer os.Remove(tmpPath)

	writer := bufio.NewWriter(file)
	for _, value := range values {
		encoded, err := json.Marshal(value)
		if err != nil {
			file.Close()
			return err
		}
		if _, err := writer.Write(append(encoded, '\n')); err != nil {
			file.Close()
			return err
		}
	}
	if err := writer.Flush(); err != nil {
		file.Close()
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	return os.Rename(tmpPath, path)
}

func writeFileAtomic(path string, data []byte) error {
	file, err := os.CreateTemp(filepath.Dir(path), ".tmp-")
	if err != nil {
		return err
	}
	tmpPath := file.Name()
	defer os.Remove(tmpPath)

	if _, err := file.Write(data); err != nil {
		file.Close()
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	return os.Rename(tmpPath, path)
}
