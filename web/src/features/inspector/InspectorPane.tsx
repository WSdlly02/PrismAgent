import type { AgentSummary, WorkspaceSession, WorkspaceSummary } from "../../api/types";

type InspectorPaneProps = {
  workspace: WorkspaceSummary | null;
  agent: AgentSummary | null;
  session: WorkspaceSession | null;
};

export function InspectorPane({ workspace, agent, session }: InspectorPaneProps) {
  return (
    <div className="inspector-pane">
      <header>
        <h2>Inspector</h2>
        <span>Workflow / Context</span>
      </header>
      <dl className="inspector-list">
        <div>
          <dt>Workspace</dt>
          <dd>{workspace?.workspace_path ?? "None"}</dd>
        </div>
        <div>
          <dt>Agent</dt>
          <dd>{agent?.agent_name ?? "None"}</dd>
        </div>
        <div>
          <dt>Workspace session</dt>
          <dd>{session ? `held by ${session.client_id}` : "None"}</dd>
        </div>
      </dl>
      <div className="inspector-empty">
        <strong>Workflow</strong>
        <p>No active workflow selected.</p>
      </div>
      <div className="inspector-empty">
        <strong>Context</strong>
        <p>No context output selected.</p>
      </div>
    </div>
  );
}
