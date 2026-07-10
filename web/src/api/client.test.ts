import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  acquireLease,
  agentSnapshot,
  createAgent,
  deleteAgent,
  listAgents,
  listProfiles,
  sendMessage,
} from "./client";

const LEASE = { workspace_uuid: "workspace-1", lease_token: "lease-1" };

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

  it("acquires workspace leases through /api/workspaces/acquire_lease", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse({
        workspace_uuid: "workspace-1",
        client_id: "client-1",
        lease_token: "lease-1",
        expires_at: 123,
      }),
    );

    await acquireLease("workspace-1", "client-1", "old-token");

    expect(fetch).toHaveBeenCalledWith("/api/workspaces/acquire_lease", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        workspace_uuid: "workspace-1",
        client_id: "client-1",
        lease_token: "old-token",
      }),
    });
  });

  it("creates agents through /api/agents/create with lease_token", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ created: true }));

    await createAgent(LEASE, {
      name: "Planner",
      profile: "planner",
      context_refs: ["ctx-in"],
      context_out: ["ctx-out"],
    });

    expect(fetch).toHaveBeenCalledWith("/api/agents/create", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        workspace_uuid: "workspace-1",
        lease_token: "lease-1",
        name: "Planner",
        profile: "planner",
        context_refs: ["ctx-in"],
        context_out: ["ctx-out"],
      }),
    });
  });

  it("deletes agents through /api/agents/delete", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(jsonResponse({ deleted: true }));

    await deleteAgent(LEASE, "agent-1");

    expect(fetch).toHaveBeenCalledWith("/api/agents/delete", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        workspace_uuid: "workspace-1",
        lease_token: "lease-1",
        agent_uuid: "agent-1",
      }),
    });
  });

  it("encodes workspace_uuid in query endpoints", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(jsonResponse([]));

    await listAgents("workspace-1");

    expect(fetch).toHaveBeenCalledWith(
      "/api/agents/list?workspace_uuid=workspace-1",
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

    const snapshot = await agentSnapshot("workspace-1", "agent-1");

    expect(fetch).toHaveBeenCalledWith(
      "/api/agents/snapshot?workspace_uuid=workspace-1&agent_uuid=agent-1",
      { headers: { "content-type": "application/json" } },
    );
    expect(snapshot.pending_approval?.request_uuid).toBe("approval-1");
  });

  it("sends message bodies with attachments array by default", async () => {
    await sendMessage(LEASE, "agent-1", "hello");

    expect(fetch).toHaveBeenCalledWith("/api/agents/send_message", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        workspace_uuid: "workspace-1",
        lease_token: "lease-1",
        agent_uuid: "agent-1",
        message_body: { text: "hello", attachments: [] },
      }),
    });
  });

  it("throws backend error messages for failed JSON responses", async () => {
    vi.mocked(fetch).mockResolvedValueOnce(
      jsonResponse({ error: "workspace locked" }, { status: 409 }),
    );

    await expect(listProfiles()).rejects.toMatchObject({
      name: "ApiError",
      message: "workspace locked",
      status: 409,
    });
  });
});
