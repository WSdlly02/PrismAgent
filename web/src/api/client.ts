import type {
  Agent,
  AgentAccess,
  AgentCreateInput,
  AgentSnapshot,
  AgentSummary,
  Lease,
  WorkspaceAccess,
  WorkspaceSummary,
} from "./types";

const JSON_HEADERS = { "content-type": "application/json" };

export function workspaceAccessQuery(access: WorkspaceAccess) {
  return new URLSearchParams({
    workspace_uuid: access.workspace_uuid,
    client_id: access.client_id,
    lease_token: access.lease_token,
  });
}

function agentAccessQuery(access: AgentAccess) {
  const params = workspaceAccessQuery(access);
  params.set("agent_uuid", access.agent_uuid);
  return params;
}

async function apiJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    headers: JSON_HEADERS,
    ...init,
  });
  const body = (await response.json()) as unknown;
  if (!response.ok) {
    const message =
      typeof body === "object" && body !== null && "error" in body
        ? String((body as { error: unknown }).error)
        : response.statusText;
    throw new Error(message);
  }
  return body as T;
}

export function listWorkspaces() {
  return apiJson<WorkspaceSummary[]>("/api/workspaces/list");
}

export function addWorkspace(path: string) {
  return apiJson<WorkspaceSummary>("/api/workspaces/add", {
    method: "POST",
    body: JSON.stringify({ path }),
  });
}

export function acquireLease(
  workspace_uuid: string,
  client_id: string,
  lease_token?: string | null,
) {
  return apiJson<Lease>("/api/workspaces/acquire_lease", {
    method: "POST",
    body: JSON.stringify({ workspace_uuid, client_id, lease_token }),
  });
}

export function releaseLease(workspace_uuid: string, lease_token: string) {
  return apiJson<{ released: true }>("/api/workspaces/release_lease", {
    method: "POST",
    body: JSON.stringify({ workspace_uuid, lease_token }),
  });
}

export function listProfiles() {
  return apiJson<string[]>("/api/profiles/list");
}

export function listAgents(access: WorkspaceAccess) {
  return apiJson<AgentSummary[]>(
    `/api/agents/list?${workspaceAccessQuery(access)}`,
  );
}

export function createAgent(access: WorkspaceAccess, agent: AgentCreateInput) {
  return apiJson<Agent>("/api/agents/create", {
    method: "POST",
    body: JSON.stringify({ ...access, ...agent }),
  });
}

export function agentSnapshot(access: AgentAccess) {
  return apiJson<AgentSnapshot>(
    `/api/agents/snapshot?${agentAccessQuery(access)}`,
  );
}

export function sendMessage(
  access: AgentAccess,
  text: string,
  attachments: Array<{ data: string; filename: string; mimetype: string }> = [],
) {
  return apiJson<{ accepted: true }>("/api/agents/send_message", {
    method: "POST",
    body: JSON.stringify({
      ...access,
      message_body: { text, attachments },
    }),
  });
}

export function approveRequest(
  access: AgentAccess,
  request_uuid: string,
  approval_mask: number,
) {
  return apiJson<{ accepted: true }>("/api/agents/approve_request", {
    method: "POST",
    body: JSON.stringify({ ...access, request_uuid, approval_mask }),
  });
}

export function cancelAgent(access: AgentAccess) {
  return apiJson<{ cancelled: true }>("/api/agents/cancel", {
    method: "POST",
    body: JSON.stringify(access),
  });
}
