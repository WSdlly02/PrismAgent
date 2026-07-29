import { ChevronRight } from "lucide-react";
import {
  Fragment,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import type {
  ToolCallContent,
  ToolResponseContent,
  Unit,
} from "../../api/types";
import {
  ConversationRail,
  type ConversationAnchor,
} from "./ConversationRail";
import { MessageCopyButton } from "./MessageCopyButton";

type MessageTimelineProps = {
  units: Unit[];
  streamingText: string;
  streamingReasoningText: string;
};

type CollapsibleMessageProps = {
  children: ReactNode;
  dataRole: "reasoning" | "tool" | "tool_call";
  label: string;
  meta: ReactNode;
  onToggle: () => void;
  streaming?: boolean;
};

function CollapsibleMessage({
  children,
  dataRole,
  label,
  meta,
  onToggle,
  streaming = false,
}: CollapsibleMessageProps) {
  return (
    <details
      className={`message message-collapsible${streaming ? " message-streaming" : ""}`}
      data-role={dataRole}
      onToggle={onToggle}
    >
      <summary>
        <span className="message-summary-label">
          <ChevronRight
            aria-hidden="true"
            className="message-disclosure-icon"
            size={14}
            strokeWidth={2}
          />
          <span>{label}</span>
        </span>
        {meta}
      </summary>
      <div className="message-collapsible-content">{children}</div>
    </details>
  );
}

// ---------------------------------------------------------------------------
// Markdown → 安全的 HTML
// marked 默认透传原始 HTML，DOMPurify 负责剥离危险部分（script、事件句柄等）
// ---------------------------------------------------------------------------
function renderMd(text: string): string {
  if (!text) {
    return "";
  }
  const raw = marked.parse(text, { async: false }) as string;
  const sanitized = DOMPurify.sanitize(raw);
  const template = document.createElement("template");
  template.innerHTML = sanitized;

  template.content.querySelectorAll("table").forEach((table) => {
    const scrollArea = document.createElement("div");
    scrollArea.className = "markdown-table-scroll";
    scrollArea.setAttribute("aria-label", "Scrollable table");
    scrollArea.setAttribute("role", "region");
    scrollArea.tabIndex = 0;
    table.replaceWith(scrollArea);
    scrollArea.appendChild(table);
  });

  return template.innerHTML;
}

// ---------------------------------------------------------------------------
// 提取文本内容（跳过 ToolCall / ToolResponse / Binary）
// ---------------------------------------------------------------------------
function collectText(unit: Unit): string {
  const content = unit.content.content;
  if (typeof content === "string") {
    return content;
  }
  if (!Array.isArray(content)) {
    return "";
  }
  return content
    .filter((part) => typeof part.Text === "string")
    .map((part) => part.Text)
    .join("\n");
}

// ---------------------------------------------------------------------------
// 提取独立 reasoning 内容
// ---------------------------------------------------------------------------
function collectReasoning(unit: Unit): string[] {
  const content = unit.content.content;
  if (!Array.isArray(content)) {
    return [];
  }
  return content.flatMap((part) => {
    if (typeof part.ReasoningContent === "string") {
      return [part.ReasoningContent];
    }
    if (typeof part.reasoning_content === "string") {
      return [part.reasoning_content];
    }
    return [];
  });
}

// ---------------------------------------------------------------------------
// 提取完整工具调用与回复
// ---------------------------------------------------------------------------
function collectToolCalls(unit: Unit): ToolCallContent[] {
  const content = unit.content.content;
  if (!Array.isArray(content)) {
    return [];
  }
  return content.flatMap((part) =>
    part.ToolCall ? [part.ToolCall] : [],
  );
}

function collectToolResponses(unit: Unit): ToolResponseContent[] {
  const content = unit.content.content;
  if (!Array.isArray(content)) {
    return [];
  }
  return content.flatMap((part) =>
    part.ToolResponse ? [part.ToolResponse] : [],
  );
}

function formatStructuredPayload(value: unknown, indent = 0): string {
  const currentIndent = " ".repeat(indent);
  const childIndent = " ".repeat(indent + 2);

  if (typeof value === "string") {
    if (!value.includes("\n") && !value.includes("\r")) {
      return JSON.stringify(value) ?? '""';
    }
    const lines = value.replace(/\r\n?/g, "\n").split("\n");
    return [
      '"""',
      ...lines.map((line) => `${childIndent}${line}`),
      `${currentIndent}"""`,
    ].join("\n");
  }

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return "[]";
    }
    return [
      "[",
      ...value.map(
        (item, index) =>
          `${childIndent}${formatStructuredPayload(item, indent + 2)}${
            index === value.length - 1 ? "" : ","
          }`,
      ),
      `${currentIndent}]`,
    ].join("\n");
  }

  if (value !== null && typeof value === "object") {
    const entries = Object.entries(value);
    if (entries.length === 0) {
      return "{}";
    }
    return [
      "{",
      ...entries.map(
        ([key, entryValue], index) =>
          `${childIndent}${JSON.stringify(key)}: ${formatStructuredPayload(
            entryValue,
            indent + 2,
          )}${index === entries.length - 1 ? "" : ","}`,
      ),
      `${currentIndent}}`,
    ].join("\n");
  }

  return JSON.stringify(value) ?? String(value);
}

function formatToolResponse(content: string): string {
  try {
    const parsed: unknown = JSON.parse(content);
    if (parsed !== null && typeof parsed === "object") {
      return formatStructuredPayload(parsed);
    }
  } catch {
    // Tool output is often plain text; preserve it verbatim.
  }
  return content;
}

// ---------------------------------------------------------------------------
// 可见性过滤 & 角色判断
// ---------------------------------------------------------------------------
function isInternal(unit: Unit): boolean {
  return unit.visibility === "internal";
}

function isToolCallMessage(unit: Unit): boolean {
  const role = unit.content.role.toLowerCase();
  const content = unit.content.content;
  if (role !== "assistant") {
    return false;
  }
  if (!Array.isArray(content)) {
    return false;
  }
  return content.some((part) => part.ToolCall);
}

function isToolResponseMessage(unit: Unit): boolean {
  return unit.content.role.toLowerCase() === "tool";
}

function historyAnchorLabel(text: string): string {
  const normalized = text.replace(/\s+/g, " ").trim();
  return normalized.length > 72 ? `${normalized.slice(0, 69)}...` : normalized;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------
const JUMP_BUTTON_THRESHOLD_PX = 100;

export function MessageTimeline({
  units,
  streamingText,
  streamingReasoningText,
}: MessageTimelineProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const autoScroll = useRef(false);
  const wasStreaming = useRef(false);
  const wasNearBottomBeforeUpdate = useRef(true);
  const previousUnitCount = useRef(units.length);
  const didSyncInitialContent = useRef(false);
  const touchStartY = useRef<number | null>(null);
  const [showJumpButton, setShowJumpButton] = useState(false);
  const isStreaming = Boolean(streamingText || streamingReasoningText);

  function distanceFromBottom(el: HTMLDivElement): number {
    return el.scrollHeight - el.scrollTop - el.clientHeight;
  }

  function isNearBottom(el: HTMLDivElement): boolean {
    return distanceFromBottom(el) < JUMP_BUTTON_THRESHOLD_PX;
  }

  function updateJumpButton(el: HTMLDivElement) {
    setShowJumpButton(distanceFromBottom(el) >= JUMP_BUTTON_THRESHOLD_PX);
  }

  function handleScroll() {
    const el = containerRef.current;
    if (!el) {
      return;
    }
    updateJumpButton(el);
    const nearBottom = isNearBottom(el);
    wasNearBottomBeforeUpdate.current = nearBottom;
    if (isStreaming && nearBottom) {
      autoScroll.current = true;
    }
  }

  function pauseAutoScroll() {
    autoScroll.current = false;
    wasNearBottomBeforeUpdate.current = false;
  }

  function handleWheel(event: React.WheelEvent<HTMLDivElement>) {
    if (event.deltaY < 0) {
      pauseAutoScroll();
    }
  }

  function handleTouchStart(event: React.TouchEvent<HTMLDivElement>) {
    touchStartY.current = event.touches[0]?.clientY ?? null;
  }

  function handleTouchMove(event: React.TouchEvent<HTMLDivElement>) {
    const startY = touchStartY.current;
    const currentY = event.touches[0]?.clientY;
    if (startY == null || currentY == null) {
      return;
    }
    if (currentY > startY) {
      pauseAutoScroll();
    }
    touchStartY.current = currentY;
  }

  function scrollToBottom(behavior: ScrollBehavior) {
    wasNearBottomBeforeUpdate.current = true;
    const bottom = bottomRef.current;
    if (typeof bottom?.scrollIntoView === "function") {
      bottom.scrollIntoView({ behavior });
    }
  }

  function syncAfterScrollFrame() {
    requestAnimationFrame(() => {
      const current = containerRef.current;
      scrollToBottom("auto");
      if (current) {
        updateJumpButton(current);
      }
    });
  }

  function handleJumpToBottom() {
    autoScroll.current = true;
    setShowJumpButton(false);
    scrollToBottom("smooth");
  }

  function handleCollapsibleToggle() {
    requestAnimationFrame(() => {
      const el = containerRef.current;
      if (!el) {
        return;
      }
      if (isStreaming && autoScroll.current) {
        scrollToBottom("auto");
      }
      updateJumpButton(el);
    });
  }

  useEffect(() => {
    const el = containerRef.current;
    if (!el) {
      return;
    }

    const didAppendUnit = units.length > previousUnitCount.current;
    const appendedRole = didAppendUnit
      ? units.at(-1)?.content.role.toLowerCase()
      : null;
    const shouldFollowNewUser =
      didAppendUnit &&
      appendedRole === "user" &&
      wasNearBottomBeforeUpdate.current;
    const shouldFinishFollowingStream =
      !isStreaming &&
      wasStreaming.current &&
      didAppendUnit &&
      autoScroll.current;
    const shouldSyncInitialContent =
      !didSyncInitialContent.current && units.length > 0;
    if (shouldSyncInitialContent) {
      didSyncInitialContent.current = true;
    }
    previousUnitCount.current = units.length;

    if (isStreaming && !wasStreaming.current) {
      autoScroll.current = wasNearBottomBeforeUpdate.current;
    }
    if (!isStreaming) {
      autoScroll.current = false;
      updateJumpButton(el);
      if (
        shouldSyncInitialContent ||
        shouldFollowNewUser ||
        shouldFinishFollowingStream
      ) {
        syncAfterScrollFrame();
      }
      wasStreaming.current = isStreaming;
      return;
    }

    updateJumpButton(el);
    if (shouldSyncInitialContent || autoScroll.current) {
      syncAfterScrollFrame();
    }

    wasStreaming.current = isStreaming;
  }, [isStreaming, units, streamingText, streamingReasoningText]);

  // 过滤掉 internal 的消息
  const visibleUnits = useMemo(
    () => units.filter((unit) => !isInternal(unit)),
    [units],
  );
  const historyAnchors = useMemo(
    () =>
      visibleUnits.flatMap<ConversationAnchor>((unit) => {
        const role = unit.content.role.toLowerCase();
        if (
          (role !== "user" && role !== "assistant") ||
          isToolCallMessage(unit)
        ) {
          return [];
        }
        const text = collectText(unit);
        return text
          ? [
              {
                id: unit.uuid,
                label: historyAnchorLabel(text),
                role,
              },
            ]
          : [];
      }),
    [visibleUnits],
  );

  return (
    <div className="message-timeline-shell">
      <div
        className="message-timeline"
        id="message-timeline-scrollport"
        onScroll={handleScroll}
        onTouchMove={handleTouchMove}
        onTouchStart={handleTouchStart}
        onWheel={handleWheel}
        ref={containerRef}
      >
        <div className="message-timeline-content" ref={contentRef}>
          {visibleUnits.length === 0 &&
          !streamingText &&
          !streamingReasoningText ? (
            <div className="empty-chat">No messages</div>
          ) : null}

          {visibleUnits.map((unit) => {
            const role = unit.content.role.toLowerCase();
            const unitTime = new Date(
              unit.created_at * 1000,
            ).toLocaleTimeString();
            const reasoningBubbles = collectReasoning(unit).map(
              (reasoning, index) => (
                <CollapsibleMessage
                  dataRole="reasoning"
                  key={`${unit.uuid}-reasoning-${index}`}
                  label="reasoning"
                  meta={<time>{unitTime}</time>}
                  onToggle={handleCollapsibleToggle}
                >
                  <div
                    className="markdown-body"
                    dangerouslySetInnerHTML={{
                      __html: renderMd(reasoning),
                    }}
                  />
                </CollapsibleMessage>
              ),
            );

            // --- 工具调用消息（assistant 中含有 ToolCall）---
            if (isToolCallMessage(unit)) {
              const text = collectText(unit);
              const calls = collectToolCalls(unit);
              return (
                <Fragment key={unit.uuid}>
                  {reasoningBubbles}
                  <CollapsibleMessage
                    dataRole="tool_call"
                    label={calls.length === 1 ? "tool call" : "tool calls"}
                    meta={<time>{unitTime}</time>}
                    onToggle={handleCollapsibleToggle}
                  >
                    {text ? (
                      <div
                        className="markdown-body"
                        dangerouslySetInnerHTML={{ __html: renderMd(text) }}
                      />
                    ) : null}
                    {calls.map((call, index) => (
                      <div
                        className="tool-entry"
                        key={call.call_id || index}
                      >
                        <div className="tool-entry-meta">
                          <code>{call.fn_name}</code>
                          <span>{call.call_id}</span>
                        </div>
                        <pre className="tool-content">
                          {formatStructuredPayload(call.fn_arguments)}
                        </pre>
                      </div>
                    ))}
                  </CollapsibleMessage>
                </Fragment>
              );
            }

            // --- 工具回复消息 ---
            if (isToolResponseMessage(unit)) {
              const responses = collectToolResponses(unit);
              return (
                <Fragment key={unit.uuid}>
                  {reasoningBubbles}
                  <CollapsibleMessage
                    dataRole="tool"
                    label="tool result"
                    meta={<time>{unitTime}</time>}
                    onToggle={handleCollapsibleToggle}
                  >
                    {responses.map((response, index) => (
                      <div
                        className="tool-entry"
                        key={response.call_id || index}
                      >
                        <div className="tool-entry-meta">
                          <code>{response.fn_name ?? "tool"}</code>
                          <span>{response.call_id}</span>
                        </div>
                        <pre className="tool-content">
                          {formatToolResponse(response.content)}
                        </pre>
                      </div>
                    ))}
                  </CollapsibleMessage>
                </Fragment>
              );
            }

            // --- 普通消息（user / assistant）渲染 Markdown ---
            const text = collectText(unit);
            const isHistoryAnchor = role === "user" || role === "assistant";
            return (
              <Fragment key={unit.uuid}>
                {reasoningBubbles}
                {text ? (
                  <article
                    className="message"
                    data-history-anchor={isHistoryAnchor ? unit.uuid : undefined}
                    data-role={role}
                  >
                    <header>
                      <span>{role}</span>
                      <div className="message-meta">
                        {isHistoryAnchor ? (
                          <MessageCopyButton text={text} />
                        ) : null}
                        <time>{unitTime}</time>
                      </div>
                    </header>
                    <div
                      className="markdown-body"
                      dangerouslySetInnerHTML={{ __html: renderMd(text) }}
                    />
                  </article>
                ) : null}
              </Fragment>
            );
          })}

          {streamingReasoningText ? (
            <CollapsibleMessage
              dataRole="reasoning"
              label="reasoning"
              meta={<span>streaming</span>}
              onToggle={handleCollapsibleToggle}
              streaming
            >
              <div
                className="markdown-body"
                dangerouslySetInnerHTML={{
                  __html: renderMd(streamingReasoningText),
                }}
              />
            </CollapsibleMessage>
          ) : null}

          {streamingText ? (
            <article className="message message-streaming" data-role="assistant">
              <header>
                <span>assistant</span>
                <time>streaming</time>
              </header>
              <div
                className="markdown-body"
                dangerouslySetInnerHTML={{ __html: renderMd(streamingText) }}
              />
            </article>
          ) : null}

          <div ref={bottomRef} />
        </div>
      </div>

      <ConversationRail
        anchors={historyAnchors}
        containerRef={containerRef}
        contentRef={contentRef}
        onManualNavigate={pauseAutoScroll}
      />

      {showJumpButton ? (
        <button
          aria-label="Jump to bottom"
          className="jump-to-bottom"
          onClick={handleJumpToBottom}
          type="button"
        >
          ↓
        </button>
      ) : null}
    </div>
  );
}
