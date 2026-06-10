export type WorkspaceSummary = {
  workspace_uuid: string;
  workspace_path: string;
  locked_by: string | null;
};

export type WorkspaceSession = {
  workspace_uuid: string;
  client_id: string;
  connected: boolean;
};

export type WorkspaceAccess = {
  workspace_uuid: string;
  client_id: string;
};

export type AgentAccess = WorkspaceAccess & {
  agent_uuid: string;
};

export type AgentStatus =
  | "idle"
  | "running_llm"
  | "running_tool"
  | "waiting_approval";

export type AgentSummary = {
  agent_uuid: string;
  agent_name: string;
  profile: string;
  auto_loop: boolean;
  context_refs: string[];
  context_out: string[];
  status: AgentStatus;
};

export type ChatPart = {
  Text?: string;
  text?: string;
  [key: string]: unknown;
};

export type ChatContent = {
  role: string;
  content?: ChatPart[] | string;
  [key: string]: unknown;
};

export type Unit = {
  uuid: string;
  visibility: "internal" | "public";
  content: ChatContent;
  token_usage: unknown | null;
  metadata: Record<string, string>;
  created_at: number;
};

export type AgentSnapshot = {
  units: Unit[];
  status: AgentStatus;
};

export type AgentCreateInput = {
  name: string;
  profile: string;
  context_refs: string[];
  context_out: string[];
};

export type Agent = {
  uuid: string;
  name: string;
  profile: string;
  auto_loop: boolean;
  auto_loop_message: string;
  unit_chain: string[];
  unit_head: string;
  context_refs: string[];
  context_out: string[];
  snapshots: Record<string, string[]>;
  created_at: number;
  updated_at: number;
};

export type PendingApproval = {
  request_uuid: string;
  description: string;
  tool_count: number;
  auto_approved_mask: number;
  manual_approval_mask: number;
};

export type AgentEvent =
  | { type: "unit_append"; unit: Unit }
  | { type: "stream_delta"; text: string }
  | { type: "approve_request"; request: PendingApproval }
  | { type: "status_changed"; status: AgentStatus }
  | { type: "error"; message: string };

export type WorkspaceEvent =
  | { type: "agent_created"; agent: AgentSummary }
  | { type: "agent_updated"; agent: AgentSummary }
  | { type: "agent_status_changed"; agent_uuid: string; status: AgentStatus }
  | { type: "agent_deleted"; agent_uuid: string }
  | {
      type: "context_created";
      context_uuid: string;
      title: string;
    }
  | {
      type: "workflow_created";
      workflow_uuid: string;
      title: string;
    }
  | {
      type: "workflow_started";
      workflow_uuid: string;
      coordinator_agent_uuid: string;
    }
  | {
      type: "workflow_cancel_requested";
      workflow_uuid: string;
      coordinator_agent_uuid: string;
    }
  | { type: "error"; message: string };
