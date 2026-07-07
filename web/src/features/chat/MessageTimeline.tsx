import { Fragment, useEffect, useRef } from "react";
import { marked } from "marked";
import DOMPurify from "dompurify";
import type { Unit } from "../../api/types";

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

function collectReasoningText(unit: Unit): string {
  const content = unit.content.content;
  if (!Array.isArray(content)) {
    return "";
  }
  return content
    .map((part) => {
      if (typeof part.ReasoningContent === "string") {
        return part.ReasoningContent;
      }
      if (typeof part.reasoning_content === "string") {
        return part.reasoning_content;
      }
      return "";
    })
    .filter(Boolean)
    .join("\n");
}

function extractPublicReasoningBlocks(text: string): string[] {
  const blocks: string[] = [];
  const pattern = /<pub>([\s\S]*?)<\/pub>/g;
  for (const match of text.matchAll(pattern)) {
    const content = match[1].trim();
    if (content) {
      blocks.push(content);
    }
  }
  return blocks;
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

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------
export function MessageTimeline({
  units,
  streamingText,
  streamingReasoningText,
}: MessageTimelineProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const isNearBottom = useRef(true);

  function handleScroll() {
    const el = containerRef.current;
    if (!el) {
      return;
    }
    isNearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100;
  }

  useEffect(() => {
    const bottom = bottomRef.current;
    if (isNearBottom.current && typeof bottom?.scrollIntoView === "function") {
      bottom.scrollIntoView({ behavior: "smooth" });
    }
  }, [units, streamingText, streamingReasoningText]);

  // 过滤掉 internal 的消息
  const visibleUnits = units.filter((u) => !isInternal(u));
  const streamingReasoningBlocks = extractPublicReasoningBlocks(streamingReasoningText);

  return (
    <div className="message-timeline" onScroll={handleScroll} ref={containerRef}>
      {visibleUnits.length === 0 && !streamingText && streamingReasoningBlocks.length === 0 ? (
        <div className="empty-chat">No messages</div>
      ) : null}

      {visibleUnits.map((unit) => {
        const role = unit.content.role.toLowerCase();
        const reasoningBlocks = extractPublicReasoningBlocks(collectReasoningText(unit));
        const reasoningMessages = reasoningBlocks.map((block, index) => (
          <article
            className="message"
            data-role="reasoning"
            key={`${unit.uuid}-reasoning-${index}`}
          >
            <header>
              <span>reasoning</span>
              <time>{new Date(unit.created_at * 1000).toLocaleTimeString()}</time>
            </header>
            <div
              className="markdown-body"
              dangerouslySetInnerHTML={{ __html: renderMd(block) }}
            />
          </article>
        ));

        // --- 工具调用消息（assistant 中含有 ToolCall）---
        if (isToolCallMessage(unit)) {
          const text = collectText(unit);
          const calls = toolCallSummary(unit);
          return (
            <Fragment key={unit.uuid}>
              {reasoningMessages}
              <article className="message" data-role="tool_call">
                <header>
                  <span>tool calls</span>
                  <time>{new Date(unit.created_at * 1000).toLocaleTimeString()}</time>
                </header>
                {text ? (
                  <div
                    className="markdown-body"
                    dangerouslySetInnerHTML={{ __html: renderMd(text) }}
                  />
                ) : null}
                {calls.map((c, i) => (
                  <pre className="tool-summary" key={i}>{c}</pre>
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
              {reasoningMessages}
              <article className="message" data-role="tool">
                <header>
                  <span>tool result</span>
                  <time>{new Date(unit.created_at * 1000).toLocaleTimeString()}</time>
                </header>
                {summaries.map((s, i) => (
                  <pre className="tool-summary" key={i}>{s}</pre>
                ))}
              </article>
            </Fragment>
          );
        }

        // --- 普通消息（user / assistant）渲染 Markdown ---
        const text = collectText(unit);
        return (
          <Fragment key={unit.uuid}>
            {reasoningMessages}
            {text ? (
              <article className="message" data-role={role}>
                <header>
                  <span>{role}</span>
                  <time>{new Date(unit.created_at * 1000).toLocaleTimeString()}</time>
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

      {streamingReasoningBlocks.map((block, index) => (
        <article
          className="message message-streaming"
          data-role="reasoning"
          key={`streaming-reasoning-${index}`}
        >
          <header>
            <span>reasoning</span>
            <time>streaming</time>
          </header>
          <div
            className="markdown-body"
            dangerouslySetInnerHTML={{ __html: renderMd(block) }}
          />
        </article>
      ))}

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
  );
}
