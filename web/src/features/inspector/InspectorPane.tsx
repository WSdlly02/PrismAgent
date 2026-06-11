import type { AgentSummary, WorkspaceSummary } from "../../api/types";

type InspectorPaneProps = {
  workspace: WorkspaceSummary | null;
  agent: AgentSummary | null;
  session: { workspace_uuid: string } | null;
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
          <dt>Session</dt>
          <dd>{session ? `subscribed to ${session.workspace_uuid}` : "None"}</dd>
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
