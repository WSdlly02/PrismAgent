import { workspaceAccessQuery } from "./client";
import type { AgentAccess, AgentEvent } from "./types";

export type ConnectionEvent = { type: "connected" };

export type RawAgentEvent = {
  eventName: string;
  data: string;
};

export function agentEventStreamUrl(access: AgentAccess) {
  const params = workspaceAccessQuery(access);
  params.set("agent_uuid", access.agent_uuid);
  return `/api/agents/event_stream?${params}`;
}

export function normalizeAgentEvent(
  raw: RawAgentEvent,
): AgentEvent | ConnectionEvent {
  if (raw.eventName === "connected") {
    return { type: "connected" };
  }

  try {
    const payload = JSON.parse(raw.data) as Record<string, unknown>;
    switch (raw.eventName) {
      case "unit_append":
        return { type: "unit_append", unit: payload.unit as AgentEventUnit };
      case "stream_delta":
        return { type: "stream_delta", text: String(payload.text ?? "") };
      case "approve_request":
        return {
          type: "approve_request",
          request: payload.request as AgentEventApproval,
        };
      case "status_changed":
        return {
          type: "status_changed",
          status: payload.status as AgentEventStatus,
        };
      case "error":
        return { type: "error", message: String(payload.message ?? raw.data) };
      default:
        return { type: "error", message: `unknown event: ${raw.eventName}` };
    }
  } catch {
    return {
      type: "error",
      message: `failed to parse ${raw.eventName} event`,
    };
  }
}

type AgentEventUnit = Extract<AgentEvent, { type: "unit_append" }>["unit"];
type AgentEventApproval = Extract<
  AgentEvent,
  { type: "approve_request" }
>["request"];
type AgentEventStatus = Extract<
  AgentEvent,
  { type: "status_changed" }
>["status"];
