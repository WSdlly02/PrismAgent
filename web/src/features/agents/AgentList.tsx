import type { AgentSummary } from "../../api/types";

type AgentListProps = {
  agents: AgentSummary[];
  selectedAgentUuid: string | null;
  onSelectAgent: (agent: AgentSummary) => void;
};

export function AgentList({ agents, selectedAgentUuid, onSelectAgent }: AgentListProps) {
  return (
    <div className="agent-list">
      {agents.length === 0 ? (
        <p className="empty-copy">No agents</p>
      ) : (
        agents.map((agent) => (
          <button
            className="resource-row"
            data-active={agent.agent_uuid === selectedAgentUuid}
            key={agent.agent_uuid}
            onClick={() => onSelectAgent(agent)}
            type="button"
          >
            <span className="resource-main">
              <span className="resource-name">{agent.agent_name}</span>
              <span className="resource-meta">{agent.profile}</span>
            </span>
            <span className={`status-dot status-${agent.status}`} aria-label={agent.status} />
          </button>
        ))
      )}
    </div>
  );
}