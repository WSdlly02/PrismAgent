import type { AgentSummary, PendingApproval, Unit } from "../../api/types";
import { approvalMaskForManual } from "../../state/approval";
import { ApprovalCard } from "./ApprovalCard";
import { ChatComposer } from "./ChatComposer";
import { MessageTimeline } from "./MessageTimeline";

type ChatPaneProps = {
  agent: AgentSummary | null;
  units: Unit[];
  streamingText: string;
  streamingReasoningText: string;
  pendingApproval: PendingApproval | null;
  statusLabel: string;
  connectionStatus: string;
  error: string | null;
  onSend: (text: string) => Promise<void>;
  onCancel: () => Promise<void>;
  onApprove: (approvalMask: number) => Promise<void>;
};

export function ChatPane({
  agent,
  units,
  streamingText,
  streamingReasoningText,
  pendingApproval,
  statusLabel,
  connectionStatus,
  error,
  onSend,
  onCancel,
  onApprove
}: ChatPaneProps) {
  return (
    <div className="chat-pane">
      <header className="chat-header">
        <div>
          <h2>{agent?.agent_name ?? "No agent selected"}</h2>
          <span>{agent?.profile ?? "Select or create an agent"}</span>
        </div>
        <div className="chat-status">
          <span className={`status-dot status-${statusLabel}`} />
          <span>{statusLabel}</span>
          <span className="connection-pill">{connectionStatus}</span>
        </div>
      </header>

      {error ? <div className="error-banner">{error}</div> : null}

      <MessageTimeline
        key={agent?.agent_uuid ?? "no-agent"}
        units={units}
        streamingReasoningText={streamingReasoningText}
        streamingText={streamingText}
      />

      {pendingApproval ? (
        <ApprovalCard
          request={pendingApproval}
          onApprove={() => void onApprove(approvalMaskForManual(pendingApproval.manual_approval_mask))}
          onDeny={() => void onApprove(0)}
        />
      ) : null}

      <ChatComposer
        hasAgent={Boolean(agent)}
        onCancel={onCancel}
        onSend={onSend}
        statusLabel={statusLabel}
      />
    </div>
  );
}
