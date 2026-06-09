import { describe, expect, it } from "vitest";
import {
  agentEventStreamUrl,
  normalizeAgentEvent,
  type RawAgentEvent,
} from "./events";
import type { AgentAccess } from "./types";

const access: AgentAccess = {
  workspace_uuid: "workspace-1",
  client_id: "client-1",
  lease_token: "lease with space",
  agent_uuid: "agent-1",
};

describe("agent events", () => {
  it("builds the event stream URL from agent access", () => {
    expect(agentEventStreamUrl(access)).toBe(
      "/api/agents/event_stream?workspace_uuid=workspace-1&client_id=client-1&lease_token=lease+with+space&agent_uuid=agent-1",
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
});
