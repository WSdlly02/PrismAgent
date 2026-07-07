import { useRef, useState } from "react";
import type { AgentSummary, PendingApproval, Unit } from "../../api/types";
import { approvalMaskForManual } from "../../state/approval";
import { ApprovalCard } from "./ApprovalCard";
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
  const [draft, setDraft] = useState("");
  const lastSentRef = useRef("");
  const isRunningLlm = statusLabel === "running_llm";
  const canCancel = isRunningLlm || statusLabel === "running_tool" || statusLabel === "waiting_approval";

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (canCancel) {
      await onCancel();
      if (isRunningLlm) {
        setDraft(lastSentRef.current);
      }
      lastSentRef.current = "";
      return;
    }
    const text = draft.trim();
    if (!text || !agent) {
      return;
    }
    lastSentRef.current = text;
    setDraft("");
    try {
      await onSend(text);
    } catch {
      setDraft(text);
      lastSentRef.current = "";
    }
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
        </div>
      </header>

      {error ? <div className="error-banner">{error}</div> : null}

      <MessageTimeline
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

      <form className="composer" onSubmit={submit}>
        <textarea
          disabled={!agent || canCancel}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Send a message"
          rows={3}
          value={draft}
        />
        <button
          className={canCancel ? "secondary-button" : "primary-button"}
          disabled={!canCancel && (!agent || !draft.trim())}
          type="submit"
        >
          {canCancel ? "Cancel" : "Send"}
        </button>
      </form>
    </div>
  );
}
