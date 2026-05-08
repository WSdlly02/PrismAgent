package core

import "time"

type UnitKind string

const (
	UnitMessage    UnitKind = "message"
	UnitLLMResp    UnitKind = "llm_response"
	UnitToolCall   UnitKind = "tool_call"
	UnitToolResult UnitKind = "tool_result"
	UnitSpawn      UnitKind = "spawn"
	UnitResult     UnitKind = "result"
)

type UnitRole string

const (
	RoleSystem    UnitRole = "system"
	RoleUser      UnitRole = "user"
	RoleAssistant UnitRole = "assistant"
	RoleTool      UnitRole = "tool"
)

type UnitScope string

const (
	ScopeWorkspace UnitScope = "workspace"
	ScopeRun       UnitScope = "run"
	ScopeAgent     UnitScope = "agent"
)

type UnitVisibility string

const (
	VisibilityUser     UnitVisibility = "user"
	VisibilityInternal UnitVisibility = "internal"
)

type Unit struct {
	UUID       string            `json:"uuid"`
	AtomHash   string            `json:"atom_hash"`
	Kind       UnitKind          `json:"kind"`
	Role       UnitRole          `json:"role"`
	Scope      UnitScope         `json:"scope"`
	Visibility UnitVisibility    `json:"visibility"`
	Metadata   map[string]string `json:"metadata,omitempty"`
	CreatedAt  time.Time         `json:"created_at"`
}

type AgentChain struct {
	AgentID  string   `json:"agent_id"`
	Chain    []string `json:"chain"`
	Head     string   `json:"head"`
	Children []string `json:"children"`
}

type Snapshot struct {
	UUID       string              `json:"uuid"`
	AgentHeads map[string]string   `json:"agent_heads"`
	UnitChains map[string][]string `json:"unit_chains"`
	CreatedAt  time.Time           `json:"created_at"`
}
