import { describe, expect, it } from "vitest";
import {
  agentEventStreamUrl,
  normalizeAgentEvent,
  normalizeWorkspaceEvent,
  type RawAgentEvent,
  workspaceEventStreamUrl,
} from "./events";
import type { AgentAccess, WorkspaceAccess } from "./types";

const workspaceAccess: WorkspaceAccess = {
  workspace_uuid: "workspace-1",
  client_id: "client-1",
};

const access: AgentAccess = {
  ...workspaceAccess,
  agent_uuid: "agent-1",
};

describe("agent events", () => {
  it("builds the event stream URL from agent access", () => {
    expect(agentEventStreamUrl(access)).toBe(
      "/api/agents/event_stream?workspace_uuid=workspace-1&client_id=client-1&agent_uuid=agent-1",
    );
  });

  it("builds the workspace event stream URL from workspace access", () => {
    expect(workspaceEventStreamUrl(workspaceAccess)).toBe(
      "/api/workspaces/event_stream?workspace_uuid=workspace-1&client_id=client-1",
    );
  });

  it("normalizes named SSE payloads into typed agent events", () => {
    const raw: RawAgentEvent = {
      eventName: "stream_delta",
      data: JSON.stringify({ text: "hello" }),
    };

    expect(normalizeAgentEvent(raw)).toEqual({
      type: "stream_delta",
      text: "hello",
    });
  });

  it("normalizes connected events separately from agent events", () => {
    expect(
      normalizeAgentEvent({
        eventName: "connected",
        data: JSON.stringify({ status: "connected" }),
      }),
    ).toEqual({ type: "connected" });
  });

  it("returns an error event for malformed payloads", () => {
    expect(
      normalizeAgentEvent({
        eventName: "unit_append",
        data: "{bad json",
      }),
    ).toEqual({
      type: "error",
      message: "failed to parse unit_append event",
    });
  });

  it("normalizes workspace agent creation events", () => {
    expect(
      normalizeWorkspaceEvent({
        eventName: "agent_created",
        data: JSON.stringify({
          agent: {
            agent_uuid: "agent-1",
            agent_name: "Planner",
            profile: "planner",
            auto_loop: false,
            context_refs: [],
            context_out: [],
            status: "idle",
          },
        }),
      }),
    ).toEqual({
      type: "agent_created",
      agent: {
        agent_uuid: "agent-1",
        agent_name: "Planner",
        profile: "planner",
        auto_loop: false,
        context_refs: [],
        context_out: [],
        status: "idle",
      },
    });
  });

  it("normalizes workspace status and resource events", () => {
    expect(
      normalizeWorkspaceEvent({
        eventName: "agent_status_changed",
        data: JSON.stringify({ agent_uuid: "agent-1", status: "running_llm" }),
      }),
    ).toEqual({
      type: "agent_status_changed",
      agent_uuid: "agent-1",
      status: "running_llm",
    });

    expect(
      normalizeWorkspaceEvent({
        eventName: "context_created",
        data: JSON.stringify({
          context_uuid: "ctx-1",
          title: "Context",
        }),
      }),
    ).toEqual({
      type: "context_created",
      context_uuid: "ctx-1",
      title: "Context",
    });
  });
});
