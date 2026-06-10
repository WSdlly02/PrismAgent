import type {
  AgentAccess,
  AgentCreateInput,
  AgentSnapshot,
  AgentSummary,
  WorkspaceAccess,
  WorkspaceSummary,
} from "./types";

const JSON_HEADERS = { "content-type": "application/json" };

export function workspaceAccessQuery(access: WorkspaceAccess) {
  return new URLSearchParams({
    workspace_uuid: access.workspace_uuid,
    client_id: access.client_id,
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

export function listProfiles() {
  return apiJson<string[]>("/api/profiles/list");
}

export function listAgents(access: WorkspaceAccess) {
  return apiJson<AgentSummary[]>(
    `/api/agents/list?${workspaceAccessQuery(access)}`,
  );
}

export function createAgent(access: WorkspaceAccess, agent: AgentCreateInput) {
  return apiJson<{ created: true }>("/api/agents/create", {
    method: "POST",
    body: JSON.stringify({ ...access, ...agent }),
  });
}

export function deleteAgent(access: AgentAccess) {
  return apiJson<{ deleted: true }>("/api/agents/delete", {
    method: "POST",
    body: JSON.stringify(access),
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
