import { Check, Copy, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

type CopyState = "idle" | "copied" | "failed";

type MessageCopyButtonProps = {
  text: string;
};

const FEEDBACK_DURATION_MS = 1800;

async function writeText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Fall back for remote HTTP sessions where Clipboard API access is denied.
    }
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.readOnly = true;
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();

  let copied = false;
  try {
    copied = document.execCommand("copy");
  } finally {
    textarea.remove();
  }
  if (!copied) {
    throw new Error("Clipboard write failed");
  }
}

export function MessageCopyButton({ text }: MessageCopyButtonProps) {
  const [state, setState] = useState<CopyState>("idle");
  const resetTimer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (resetTimer.current != null) {
        window.clearTimeout(resetTimer.current);
      }
    },
    [],
  );

  async function handleCopy() {
    if (resetTimer.current != null) {
      window.clearTimeout(resetTimer.current);
    }

    try {
      await writeText(text);
      setState("copied");
    } catch {
      setState("failed");
    }

    resetTimer.current = window.setTimeout(() => {
      setState("idle");
      resetTimer.current = null;
    }, FEEDBACK_DURATION_MS);
  }

  const label =
    state === "copied"
      ? "Copied"
      : state === "failed"
        ? "Copy failed"
        : "Copy message";
  const Icon = state === "copied" ? Check : state === "failed" ? X : Copy;

  return (
    <button
      aria-label={label}
      aria-live="polite"
      className="message-copy-button"
      data-copy-state={state}
      onClick={handleCopy}
      title={label}
      type="button"
    >
      <Icon aria-hidden="true" size={14} strokeWidth={2} />
    </button>
  );
}
