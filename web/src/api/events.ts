import { workspaceAccessQuery } from "./client";
import type { AgentAccess, AgentEvent, WorkspaceAccess, WorkspaceEvent } from "./types";

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

export function workspaceEventStreamUrl(access: WorkspaceAccess) {
  return `/api/workspaces/event_stream?${workspaceAccessQuery(access)}`;
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

export function normalizeWorkspaceEvent(
  raw: RawAgentEvent,
): WorkspaceEvent | ConnectionEvent {
  if (raw.eventName === "connected") {
    return { type: "connected" };
  }

  try {
    const payload = JSON.parse(raw.data) as Record<string, unknown>;
    switch (raw.eventName) {
      case "agent_created":
        return { type: "agent_created", agent: payload.agent as WorkspaceEventAgent };
      case "agent_updated":
        return { type: "agent_updated", agent: payload.agent as WorkspaceEventAgent };
      case "agent_status_changed":
        return {
          type: "agent_status_changed",
          agent_uuid: String(payload.agent_uuid ?? ""),
          status: payload.status as WorkspaceEventStatus,
        };
      case "agent_deleted":
        return { type: "agent_deleted", agent_uuid: String(payload.agent_uuid ?? "") };
      case "context_created":
        return {
          type: "context_created",
          context_uuid: String(payload.context_uuid ?? ""),
          title: String(payload.title ?? ""),
        };
      case "workflow_created":
        return {
          type: "workflow_created",
          workflow_uuid: String(payload.workflow_uuid ?? ""),
          title: String(payload.title ?? ""),
        };
      case "workflow_started":
        return {
          type: "workflow_started",
          workflow_uuid: String(payload.workflow_uuid ?? ""),
          coordinator_agent_uuid: String(payload.coordinator_agent_uuid ?? ""),
        };
      case "workflow_cancel_requested":
        return {
          type: "workflow_cancel_requested",
          workflow_uuid: String(payload.workflow_uuid ?? ""),
          coordinator_agent_uuid: String(payload.coordinator_agent_uuid ?? ""),
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
type WorkspaceEventAgent = Extract<WorkspaceEvent, { type: "agent_created" }>["agent"];
type WorkspaceEventStatus = Extract<
  WorkspaceEvent,
  { type: "agent_status_changed" }
>["status"];
