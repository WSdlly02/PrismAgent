import type {
  AgentCreateInput,
  AgentSnapshot,
  AgentSummary,
  Lease,
  PublicError,
  WorkspaceLease,
  WorkspaceSummary,
} from "./types";

const JSON_HEADERS = { "content-type": "application/json" };

export class ApiError extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly code: string = "unknown_error",
    public readonly retryable: boolean = false,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function apiJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, {
    headers: JSON_HEADERS,
    ...init,
  });
  const body = (await response.json()) as unknown;
  if (!response.ok) {
    const error =
      typeof body === "object" && body !== null && "error" in body
        ? (body as { error: unknown }).error
        : null;
    if (
      typeof error === "object" &&
      error !== null &&
      "message" in error &&
      typeof error.message === "string"
    ) {
      const publicError = error as Partial<PublicError> & { message: string };
      throw new ApiError(
        publicError.message,
        response.status,
        publicError.code ?? "unknown_error",
        publicError.retryable ?? false,
      );
    }
    // Accept the previous string envelope during rolling frontend/backend updates.
    throw new ApiError(
      typeof error === "string" ? error : response.statusText,
      response.status,
    );
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

export function listAgents(workspaceUuid: string) {
  return apiJson<AgentSummary[]>(
    `/api/agents/list?workspace_uuid=${encodeURIComponent(workspaceUuid)}`,
  );
}

export function createAgent(access: WorkspaceLease, agent: AgentCreateInput) {
  return apiJson<{ created: true }>("/api/agents/create", {
    method: "POST",
    body: JSON.stringify({ ...access, ...agent }),
  });
}

export function deleteAgent(access: WorkspaceLease, agentUuid: string) {
  return apiJson<{ deleted: true }>("/api/agents/delete", {
    method: "POST",
    body: JSON.stringify({
      ...access,
      agent_uuid: agentUuid,
    }),
  });
}

export function agentSnapshot(workspaceUuid: string, agentUuid: string) {
  return apiJson<AgentSnapshot>(
    `/api/agents/snapshot?workspace_uuid=${encodeURIComponent(workspaceUuid)}&agent_uuid=${encodeURIComponent(agentUuid)}`,
  );
}

export function sendMessage(
  access: WorkspaceLease,
  agentUuid: string,
  text: string,
  attachments: Array<{ data: string; filename: string; mimetype: string }> = [],
) {
  return apiJson<{ accepted: true }>("/api/agents/send_message", {
    method: "POST",
    body: JSON.stringify({
      ...access,
      agent_uuid: agentUuid,
      message_body: { text, attachments },
    }),
  });
}

export function deleteWorkspace(access: WorkspaceLease) {
  return apiJson<{ deleted: true }>("/api/workspaces/delete", {
    method: "POST",
    body: JSON.stringify(access),
  });
}

export function approveRequest(
  access: WorkspaceLease,
  agentUuid: string,
  request_uuid: string,
  approval_mask: number,
) {
  return apiJson<{ accepted: true }>("/api/agents/approve_request", {
    method: "POST",
    body: JSON.stringify({
      ...access,
      agent_uuid: agentUuid,
      request_uuid,
      approval_mask,
    }),
  });
}

export function cancelAgent(access: WorkspaceLease, agentUuid: string) {
  return apiJson<{ cancelled: true }>("/api/agents/cancel", {
    method: "POST",
    body: JSON.stringify({
      ...access,
      agent_uuid: agentUuid,
    }),
  });
}
