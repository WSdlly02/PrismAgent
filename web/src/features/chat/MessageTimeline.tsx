import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import type { Unit } from "../../api/types";
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

// ---------------------------------------------------------------------------
// Markdown → 安全的 HTML
// marked 默认透传原始 HTML，DOMPurify 负责剥离危险部分（script、事件句柄等）
// ---------------------------------------------------------------------------
function renderMd(text: string): string {
  if (!text) {
    return "";
  }
  const raw = marked.parse(text, { async: false }) as string;
  return DOMPurify.sanitize(raw);
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
// 工具调用摘要：🔧 fn_name(args…)
// ---------------------------------------------------------------------------
function toolCallSummary(unit: Unit): string[] {
  const content = unit.content.content;
  if (!Array.isArray(content)) {
    return [];
  }
  return content
    .filter((part) => part.ToolCall)
    .map((part) => {
      const tc = part.ToolCall as
        | { fn_name?: string; fn_arguments?: Record<string, unknown> }
        | undefined;
      if (!tc) {
        return "";
      }
      const args = tc.fn_arguments ?? {};
      const argsStr = Object.keys(args).length
        ? Object.entries(args)
            .map(([k, v]) => `${k}: ${String(v).slice(0, 60)}`)
            .join(", ")
        : "";
      return `🔧 ${tc.fn_name}(${argsStr})`;
    })
    .filter(Boolean);
}

// ---------------------------------------------------------------------------
// 工具回复摘要：📥 fn_name → content（截断）
// ---------------------------------------------------------------------------
function toolResponseSummary(unit: Unit): string[] {
  const content = unit.content.content;
  if (!Array.isArray(content)) {
    return [];
  }
  return content
    .filter((part) => part.ToolResponse)
    .map((part) => {
      const tr = part.ToolResponse as
        | { call_id?: string; content?: string }
        | undefined;
      if (!tr) {
        return "";
      }
      const snippet = (tr.content ?? "").slice(0, 120);
      return `📥 ${snippet}`;
    })
    .filter(Boolean);
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

            // --- 工具调用消息（assistant 中含有 ToolCall）---
            if (isToolCallMessage(unit)) {
              const text = collectText(unit);
              const calls = toolCallSummary(unit);
              return (
                <Fragment key={unit.uuid}>
                  <article className="message" data-role="tool_call">
                    <header>
                      <span>tool calls</span>
                      <time>
                        {new Date(unit.created_at * 1000).toLocaleTimeString()}
                      </time>
                    </header>
                    {text ? (
                      <div
                        className="markdown-body"
                        dangerouslySetInnerHTML={{ __html: renderMd(text) }}
                      />
                    ) : null}
                    {calls.map((c, i) => (
                      <pre className="tool-summary" key={i}>
                        {c}
                      </pre>
                    ))}
                  </article>
                </Fragment>
              );
            }

            // --- 工具回复消息 ---
            if (isToolResponseMessage(unit)) {
              const summaries = toolResponseSummary(unit);
              return (
                <Fragment key={unit.uuid}>
                  <article className="message" data-role="tool">
                    <header>
                      <span>tool result</span>
                      <time>
                        {new Date(unit.created_at * 1000).toLocaleTimeString()}
                      </time>
                    </header>
                    {summaries.map((s, i) => (
                      <pre className="tool-summary" key={i}>
                        {s}
                      </pre>
                    ))}
                  </article>
                </Fragment>
              );
            }

            // --- 普通消息（user / assistant）渲染 Markdown ---
            const text = collectText(unit);
            const isHistoryAnchor = role === "user" || role === "assistant";
            return (
              <Fragment key={unit.uuid}>
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
                        <time>
                          {new Date(unit.created_at * 1000).toLocaleTimeString()}
                        </time>
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
            <article className="message message-streaming" data-role="reasoning">
              <header>
                <span>reasoning</span>
                <time>streaming</time>
              </header>
              <div
                className="markdown-body"
                dangerouslySetInnerHTML={{
                  __html: renderMd(streamingReasoningText),
                }}
              />
            </article>
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
