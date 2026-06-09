import type { AgentSummary, Lease, WorkspaceSummary } from "../../api/types";

type InspectorPaneProps = {
  workspace: WorkspaceSummary | null;
  agent: AgentSummary | null;
  lease: Lease | null;
};

export function InspectorPane({ workspace, agent, lease }: InspectorPaneProps) {
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
          <dt>Lease</dt>
          <dd>{lease ? `expires ${new Date(lease.expires_at * 1000).toLocaleTimeString()}` : "None"}</dd>
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