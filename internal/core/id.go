package core

type WorkspaceID string
type RunID string
type TaskID string
type ContextObjectID string
type SnapshotID string

func (id WorkspaceID) String() string     { return string(id) }
func (id RunID) String() string           { return string(id) }
func (id TaskID) String() string          { return string(id) }
func (id ContextObjectID) String() string { return string(id) }
func (id SnapshotID) String() string      { return string(id) }
