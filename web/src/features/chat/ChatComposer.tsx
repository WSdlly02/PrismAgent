import { memo, useRef, useState } from "react";

type ChatComposerProps = {
  hasAgent: boolean;
  statusLabel: string;
  onSend: (text: string) => Promise<void>;
  onCancel: () => Promise<void>;
};

export const ChatComposer = memo(function ChatComposer({
  hasAgent,
  statusLabel,
  onSend,
  onCancel,
}: ChatComposerProps) {
  const [draft, setDraft] = useState("");
  const lastSentRef = useRef("");
  const isRunningLlm = statusLabel === "running_llm";
  const canCancel =
    isRunningLlm ||
    statusLabel === "running_tool" ||
    statusLabel === "waiting_approval";

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
    if (!text || !hasAgent) {
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
    <form className="composer" onSubmit={submit}>
      <textarea
        disabled={!hasAgent || canCancel}
        onChange={(event) => setDraft(event.target.value)}
        placeholder="Send a message"
        rows={3}
        value={draft}
      />
      <button
        className={canCancel ? "secondary-button" : "primary-button"}
        disabled={!canCancel && (!hasAgent || !draft.trim())}
        type="submit"
      >
        {canCancel ? "Cancel" : "Send"}
      </button>
    </form>
  );
});
