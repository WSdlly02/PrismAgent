import { useState } from "react";
import type { AgentCreateInput, AgentSummary, WorkspaceSummary } from "../../api/types";

type WorkspaceSidebarProps = {
  workspaces: WorkspaceSummary[];
  profiles: string[];
  expandedWorkspaceUuids: string[];
  workspaceAgents: Record<string, AgentSummary[]>;
  selectedAgentUuid: string | null;
  onSelectWorkspace: (workspace: WorkspaceSummary) => Promise<void>;
  onSelectAgent: (agent: AgentSummary) => Promise<void>;
  onAddWorkspace: (path: string) => Promise<void>;
  onCreateAgent: (workspaceUuid: string, input: AgentCreateInput) => Promise<void>;
};

const PROFILE_HINTS: Record<string, string> = {
  default: "通用助手，可读文件、搜索网页、调用工具",
  coordinator: "协调者，管理工作流执行与agent调度",
  planner: "规划者，将目标拆解为工作流",
  executor: "执行者，执行具体任务（自动循环）",
  verifier: "验证者，审查执行结果（自动循环）",
};

export function WorkspaceSidebar({
  workspaces,
  profiles,
  expandedWorkspaceUuids,
  workspaceAgents,
  selectedAgentUuid,
  onSelectWorkspace,
  onSelectAgent,
  onAddWorkspace,
  onCreateAgent,
}: WorkspaceSidebarProps) {
  const [workspacePath, setWorkspacePath] = useState("");
  const [creatingInWs, setCreatingInWs] = useState<string | null>(null);
  const [agentName, setAgentName] = useState("");
  const [profile, setProfile] = useState("");
  const [contextRefs, setContextRefs] = useState("");
  const [contextOut, setContextOut] = useState("");

  async function submitWorkspace(event: React.FormEvent) {
    event.preventDefault();
    const path = workspacePath.trim();
    if (!path) {
      return;
    }
    await onAddWorkspace(path);
    setWorkspacePath("");
  }

  async function submitAgent(event: React.FormEvent, wsUuid: string) {
    event.preventDefault();
    const name = agentName.trim();
    if (!name) {
      return;
    }
    const selectedProfile = profile || profiles[0] || "default";
    await onCreateAgent(wsUuid, {
      name,
      profile: selectedProfile,
      context_refs: contextRefs
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
      context_out: contextOut
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
    });
    setAgentName("");
    setContextRefs("");
    setContextOut("");
    setCreatingInWs(null);
  }

  function startCreate(wsUuid: string) {
    setCreatingInWs(wsUuid);
    setAgentName("");
    setProfile("");
    setContextRefs("");
    setContextOut("");
  }

  return (
    <div className="sidebar">
      <div className="sidebar-header">
        <div>
          <h1>PrismAgent</h1>
          <span className="muted-label">Shell</span>
        </div>
      </div>

      {/* Workspace tree */}
      <div className="workspace-tree">
        {workspaces.map((workspace) => {
          const isExpanded = expandedWorkspaceUuids.includes(workspace.workspace_uuid);
          const children = workspaceAgents[workspace.workspace_uuid] ?? [];
          const isCreating = creatingInWs === workspace.workspace_uuid;

          return (
            <div className="ws-folder" key={workspace.workspace_uuid}>
              {/* 文件夹行：双击展开/折叠 */}
              <button
                className="ws-folder-row"
                onDoubleClick={() => void onSelectWorkspace(workspace)}
                type="button"
              >
                <span className="ws-folder-icon">{isExpanded ? "📂" : "📁"}</span>
                <span className="resource-main">
                  <span className="resource-name">{workspace.workspace_path}</span>
                  <span className="resource-meta">
                    {workspace.locked_by ? `locked by ${workspace.locked_by}` : "available"}
                  </span>
                </span>
                <span className="ws-count">{children.length}</span>
              </button>

              {/* 展开后显示 agents */}
              {isExpanded ? (
                <div className="ws-children">
                  {/* Create Agent 顶部按钮 */}
                  {isCreating ? (
                    <form className="ws-agent-form" onSubmit={(e) => submitAgent(e, workspace.workspace_uuid)}>
                      <input
                        aria-label="Agent name"
                        onChange={(e) => setAgentName(e.target.value)}
                        placeholder="Agent name (required)"
                        value={agentName}
                      />
                      <div className="ws-field-with-hint">
                        <select
                          aria-label="Agent profile"
                          onChange={(e) => setProfile(e.target.value)}
                          value={profile || profiles[0] || "default"}
                        >
                          {(profiles.length ? profiles : ["default"]).map((name) => (
                            <option key={name} value={name}>{name}</option>
                          ))}
                        </select>
                        <span className="ws-field-hint">
                          {PROFILE_HINTS[profile || profiles[0] || "default"] ?? ""}
                        </span>
                      </div>

                      {/* 可选高级字段 */}
                      <details className="ws-advanced-fields">
                        <summary>Advanced (optional)</summary>
                        <input
                          aria-label="Context refs"
                          onChange={(e) => setContextRefs(e.target.value)}
                          placeholder="context_refs: uuid1, uuid2 (optional)"
                          value={contextRefs}
                        />
                        <input
                          aria-label="Context out"
                          onChange={(e) => setContextOut(e.target.value)}
                          placeholder="context_out: uuid1, uuid2 (optional)"
                          value={contextOut}
                        />
                      </details>

                      <div className="ws-agent-form-actions">
                        <button className="secondary-button" onClick={() => setCreatingInWs(null)} type="button">Cancel</button>
                        <button className="primary-button" type="submit">Create</button>
                      </div>
                    </form>
                  ) : (
                    <button className="ws-create-btn" onClick={() => startCreate(workspace.workspace_uuid)} type="button">
                      + Create Agent
                    </button>
                  )}

                  {/* Agent 列表 */}
                  {children.map((agent) => (
                    <button
                      className="ws-agent-row"
                      data-active={agent.agent_uuid === selectedAgentUuid}
                      key={agent.agent_uuid}
                      onClick={() => void onSelectAgent(agent)}
                      type="button"
                    >
                      <span className="ws-agent-icon">🤖</span>
                      <span className="resource-main">
                        <span className="resource-name">{agent.agent_name}</span>
                        <span className="resource-meta">{agent.profile}</span>
                      </span>
                      <span className={`status-dot status-${agent.status}`} aria-label={agent.status} />
                    </button>
                  ))}

                  {children.length === 0 && !isCreating ? (
                    <p className="empty-copy">No agents</p>
                  ) : null}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>

      {/* 底部：添加 workspace */}
      <div className="sidebar-footer-form">
        <form className="compact-form" onSubmit={submitWorkspace}>
          <input
            aria-label="Workspace path"
            onChange={(event) => setWorkspacePath(event.target.value)}
            placeholder="/home/user/project"
            value={workspacePath}
          />
          <button type="submit">Add</button>
        </form>
      </div>
    </div>
  );
}
