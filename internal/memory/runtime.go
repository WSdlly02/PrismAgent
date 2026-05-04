package memory

import (
	"context"
	"fmt"
	"sync"

	"prismagent/internal/core"
)

type Runtime struct {
	mu sync.RWMutex

	tasks     map[core.TaskID]core.Task
	contexts  map[core.ContextObjectID]core.ContextObject
	snapshots map[core.SnapshotID]core.SnapshotRecord
	events    []core.Event
}

func NewRuntime() *Runtime {
	return &Runtime{
		tasks:     make(map[core.TaskID]core.Task),
		contexts:  make(map[core.ContextObjectID]core.ContextObject),
		snapshots: make(map[core.SnapshotID]core.SnapshotRecord),
		events:    make([]core.Event, 0),
	}
}

func (r *Runtime) CreateTask(_ context.Context, task core.Task) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, exists := r.tasks[task.ID]; exists {
		return fmt.Errorf("task already exists: %s", task.ID)
	}
	r.tasks[task.ID] = task
	return nil
}

func (r *Runtime) GetTask(_ context.Context, id core.TaskID) (core.Task, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	task, exists := r.tasks[id]
	if !exists {
		return core.Task{}, fmt.Errorf("task not found: %s", id)
	}
	return task, nil
}

func (r *Runtime) UpdateTask(_ context.Context, task core.Task) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, exists := r.tasks[task.ID]; !exists {
		return fmt.Errorf("task not found: %s", task.ID)
	}
	r.tasks[task.ID] = task
	return nil
}

func (r *Runtime) ListTasks(_ context.Context, runID core.RunID) ([]core.Task, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	tasks := make([]core.Task, 0)
	for _, task := range r.tasks {
		if task.RunID == runID {
			tasks = append(tasks, task)
		}
	}
	return tasks, nil
}

func (r *Runtime) PutContextObject(_ context.Context, object core.ContextObject) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.contexts[object.ID] = cloneContextObject(object)
	return nil
}

func (r *Runtime) GetContextObject(_ context.Context, id core.ContextObjectID) (core.ContextObject, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	object, exists := r.contexts[id]
	if !exists {
		return core.ContextObject{}, fmt.Errorf("context object not found: %s", id)
	}
	return cloneContextObject(object), nil
}

func (r *Runtime) ListContextObjects(_ context.Context, scope core.ContextScope) ([]core.ContextObject, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	objects := make([]core.ContextObject, 0)
	for _, object := range r.contexts {
		if scopeMatches(object.Scope, scope) {
			objects = append(objects, cloneContextObject(object))
		}
	}
	return objects, nil
}

func (r *Runtime) CreateSnapshot(_ context.Context, snapshot core.Snapshot, state core.SnapshotState) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, exists := r.snapshots[snapshot.ID]; exists {
		return fmt.Errorf("snapshot already exists: %s", snapshot.ID)
	}
	r.snapshots[snapshot.ID] = core.SnapshotRecord{
		Snapshot: snapshot,
		State:    cloneSnapshotState(state),
	}
	return nil
}

func (r *Runtime) CaptureSnapshot(_ context.Context, snapshot core.Snapshot) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, exists := r.snapshots[snapshot.ID]; exists {
		return fmt.Errorf("snapshot already exists: %s", snapshot.ID)
	}
	state := core.SnapshotState{
		Tasks:          make([]core.Task, 0),
		ContextObjects: make([]core.ContextObject, 0),
	}
	for _, task := range r.tasks {
		if task.RunID == snapshot.RunID {
			state.Tasks = append(state.Tasks, task)
		}
	}
	for _, object := range r.contexts {
		if object.Scope.RunID == snapshot.RunID {
			state.ContextObjects = append(state.ContextObjects, cloneContextObject(object))
		}
	}
	r.snapshots[snapshot.ID] = core.SnapshotRecord{
		Snapshot: snapshot,
		State:    state,
	}
	return nil
}

func (r *Runtime) GetSnapshot(_ context.Context, id core.SnapshotID) (core.SnapshotRecord, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	record, exists := r.snapshots[id]
	if !exists {
		return core.SnapshotRecord{}, fmt.Errorf("snapshot not found: %s", id)
	}
	return core.SnapshotRecord{
		Snapshot: record.Snapshot,
		State:    cloneSnapshotState(record.State),
	}, nil
}

func (r *Runtime) RestoreSnapshot(_ context.Context, id core.SnapshotID) (core.SnapshotState, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	record, exists := r.snapshots[id]
	if !exists {
		return core.SnapshotState{}, fmt.Errorf("snapshot not found: %s", id)
	}
	state := cloneSnapshotState(record.State)
	for _, task := range state.Tasks {
		r.tasks[task.ID] = task
	}
	for _, object := range state.ContextObjects {
		r.contexts[object.ID] = cloneContextObject(object)
	}
	return state, nil
}

func (r *Runtime) Emit(_ context.Context, event core.Event) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.events = append(r.events, event)
	return nil
}

func (r *Runtime) Events() []core.Event {
	r.mu.RLock()
	defer r.mu.RUnlock()
	events := make([]core.Event, len(r.events))
	copy(events, r.events)
	return events
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

func cloneSnapshotState(state core.SnapshotState) core.SnapshotState {
	tasks := make([]core.Task, len(state.Tasks))
	copy(tasks, state.Tasks)
	objects := make([]core.ContextObject, len(state.ContextObjects))
	for i, object := range state.ContextObjects {
		objects[i] = cloneContextObject(object)
	}
	return core.SnapshotState{
		Tasks:          tasks,
		ContextObjects: objects,
	}
}

func cloneContextObject(object core.ContextObject) core.ContextObject {
	if object.Tags == nil {
		return object
	}
	tags := make([]string, len(object.Tags))
	copy(tags, object.Tags)
	object.Tags = tags
	return object
}
