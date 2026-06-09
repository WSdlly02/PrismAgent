import { useState } from "react";
import type { AgentSummary, PendingApproval, Unit } from "../../api/types";
import { approvalMaskForManual } from "../../state/approval";
import { ApprovalCard } from "./ApprovalCard";
import { MessageTimeline } from "./MessageTimeline";

type ChatPaneProps = {
  agent: AgentSummary | null;
  units: Unit[];
  streamingText: string;
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
  pendingApproval,
  statusLabel,
  connectionStatus,
  error,
  onSend,
  onCancel,
  onApprove
}: ChatPaneProps) {
  const [draft, setDraft] = useState("");
  const isRunning = statusLabel === "running_llm" || statusLabel === "running_tool";

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    const text = draft.trim();
    if (!text || !agent) {
      return;
    }
    setDraft("");
    await onSend(text);
  }

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
          {isRunning ? (
            <button className="secondary-button" onClick={() => void onCancel()} type="button">
              Cancel
            </button>
          ) : null}
        </div>
      </header>

      {error ? <div className="error-banner">{error}</div> : null}

      <MessageTimeline units={units} streamingText={streamingText} />

      {pendingApproval ? (
        <ApprovalCard
          request={pendingApproval}
          // Notice: we are using the manual approval mask directly here, which means only providing binary approve/deny options. In the future, we could enhance this to allow more granular approvals if needed.
          onApprove={() => void onApprove(approvalMaskForManual(pendingApproval.manual_approval_mask))}
          onDeny={() => void onApprove(0)}
        />
      ) : null}

      <form className="composer" onSubmit={submit}>
        <textarea
          disabled={!agent}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Send a message"
          rows={3}
          value={draft}
        />
        <button className="primary-button" disabled={!agent || !draft.trim()} type="submit">
          Send
        </button>
      </form>
    </div>
  );
}
