import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  agentSnapshot,
  createAgent,
  deleteAgent,
  listAgents,
  listProfiles,
  sendMessage,
  workspaceAccessQuery,
} from "./client";
import type { WorkspaceAccess } from "./types";

const access: WorkspaceAccess = {
  workspace_uuid: "workspace-1",
  client_id: "client-1",
};

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
    ...init,
  });
}

describe("api client", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => jsonResponse({ ok: true })),
    );
  });

  it("lists profiles through the ShellActor profile endpoint", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse(["default", "planner"]),
    );

    await expect(listProfiles()).resolves.toEqual(["default", "planner"]);

    expect(fetch).toHaveBeenCalledWith("/api/profiles/list", {
      headers: { "content-type": "application/json" },
    });
  });

  it("creates agents through /api/agents/create with flattened access fields", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ created: true }));

    await createAgent(access, {
      name: "Planner",
      profile: "planner",
      context_refs: ["ctx-in"],
      context_out: ["ctx-out"],
    });

    expect(fetch).toHaveBeenCalledWith("/api/agents/create", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        ...access,
        name: "Planner",
        profile: "planner",
        context_refs: ["ctx-in"],
        context_out: ["ctx-out"],
      }),
    });
  });

  it("deletes agents through /api/agents/delete", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ deleted: true }));

    await deleteAgent({ ...access, agent_uuid: "agent-1" });

    expect(fetch).toHaveBeenCalledWith("/api/agents/delete", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ ...access, agent_uuid: "agent-1" }),
    });
  });

  it("encodes workspace access for query endpoints", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(jsonResponse([]));

    await listAgents(access);

    expect(fetch).toHaveBeenCalledWith(
      "/api/agents/list?workspace_uuid=workspace-1&client_id=client-1",
      { headers: { "content-type": "application/json" } },
    );
  });

  it("preserves pending approval data in agent snapshots", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse({
        units: [],
        status: "waiting_approval",
        pending_approval: {
          request_uuid: "approval-1",
          description: "model requested tool execution",
          tool_count: 2,
          auto_approved_mask: 1,
          manual_approval_mask: 2,
        },
      }),
    );

    const snapshot = await agentSnapshot({ ...access, agent_uuid: "agent-1" });

    expect(fetch).toHaveBeenCalledWith(
      "/api/agents/snapshot?workspace_uuid=workspace-1&client_id=client-1&agent_uuid=agent-1",
      { headers: { "content-type": "application/json" } },
    );
    expect(snapshot.pending_approval?.request_uuid).toBe("approval-1");
  });

  it("sends message bodies with attachments array by default", async () => {
    await sendMessage({ ...access, agent_uuid: "agent-1" }, "hello");

    expect(fetch).toHaveBeenCalledWith("/api/agents/send_message", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        ...access,
        agent_uuid: "agent-1",
        message_body: { text: "hello", attachments: [] },
      }),
    });
  });

  it("throws backend error messages for failed JSON responses", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse({ error: "workspace locked" }, { status: 409 }),
    );

    await expect(listProfiles()).rejects.toThrow("workspace locked");
  });

  it("builds query strings from workspace access", () => {
    expect(workspaceAccessQuery(access).toString()).toBe(
      "workspace_uuid=workspace-1&client_id=client-1",
    );
  });
});
