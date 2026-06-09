import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  acquireLease,
  addWorkspace as addWorkspaceApi,
  agentSnapshot,
  approveRequest,
  cancelAgent,
  createAgent as createAgentApi,
  listAgents,
  listProfiles,
  listWorkspaces,
  sendMessage,
} from "../api/client";
import { agentEventStreamUrl, normalizeAgentEvent } from "../api/events";
import type {
  AgentAccess,
  AgentCreateInput,
  AgentSummary,
  Lease,
  PendingApproval,
  Unit,
  WorkspaceAccess,
  WorkspaceSummary,
} from "../api/types";
import { shouldRenewLease } from "./lease";
import { applyAgentEvent, initialChatState } from "./sessionModel";

const AGENT_EVENT_NAMES = [
  "connected",
  "unit_append",
  "stream_delta",
  "approve_request",
  "status_changed",
  "error",
];

function createClientId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `client-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export type PrismSession = {
  clientId: string;
  workspaces: WorkspaceSummary[];
  profiles: string[];
  expandedWorkspaceUuids: string[];
  workspaceLeases: Record<string, Lease>;
  workspaceAgents: Record<string, AgentSummary[]>;
  selectedAgent: AgentSummary | null;
  selectedWorkspace: WorkspaceSummary | null;
  lease: Lease | null;
  units: Unit[];
  streamingText: string;
  pendingApproval: PendingApproval | null;
  statusLabel: string;
  connectionStatus: "idle" | "connecting" | "connected" | "error";
  error: string | null;
  loadInitialData: () => Promise<void>;
  expandWorkspace: (workspace: WorkspaceSummary) => Promise<void>;
  selectAgent: (agent: AgentSummary) => Promise<void>;
  addWorkspace: (path: string) => Promise<void>;
  createAgent: (workspaceUuid: string, input: AgentCreateInput) => Promise<void>;
  send: (text: string) => Promise<void>;
  cancel: () => Promise<void>;
  approve: (approvalMask: number) => Promise<void>;
};

/** 从 workspaceLeases 取 lease，必要时续租 */
async function ensureWsLease(
  wsUuid: string,
  clientId: string,
  leases: Record<string, Lease>,
  setLeases: React.Dispatch<React.SetStateAction<Record<string, Lease>>>,
): Promise<Lease | null> {
  const existing = leases[wsUuid];
  if (existing && !shouldRenewLease(existing)) {
    return existing;
  }
  const renewed = await acquireLease(wsUuid, clientId, existing?.lease_token ?? null);
  setLeases((prev) => ({ ...prev, [wsUuid]: renewed }));
  return renewed;
}

export function usePrismSession(): PrismSession {
  const [clientId] = useState(createClientId);
  const [workspaces, setWorkspaces] = useState<WorkspaceSummary[]>([]);
  const [profiles, setProfiles] = useState<string[]>([]);
  const [expandedWorkspaceUuids, setExpandedWorkspaceUuids] = useState<string[]>([]);
  const [workspaceLeases, setWorkspaceLeases] = useState<Record<string, Lease>>({});
  const [workspaceAgents, setWorkspaceAgents] = useState<Record<string, AgentSummary[]>>({});
  const [selectedAgent, setSelectedAgent] = useState<AgentSummary | null>(null);
  const [chat, setChat] = useState(initialChatState);
  const [connectionStatus, setConnectionStatus] =
    useState<PrismSession["connectionStatus"]>("idle");
  const [error, setError] = useState<string | null>(null);
  const streamRef = useRef<EventSource | null>(null);

  // 当前选中 agent 所属 workspace 的 lease（操作目标），不从 expandWorkspace 改变
  const activeLease = useMemo<Lease | null>(() => {
    if (!selectedAgent) {
      return null;
    }
    // 找到 agent 所属的 workspace uuid
    const wsUuid = Object.entries(workspaceAgents).find(
      ([, agents]) => agents.some((a) => a.agent_uuid === selectedAgent.agent_uuid),
    )?.[0];
    if (!wsUuid) {
      return null;
    }
    return workspaceLeases[wsUuid] ?? null;
  }, [selectedAgent, workspaceAgents, workspaceLeases]);

  const selectedWorkspace = useMemo(
    () => workspaces.find((ws) => ws.workspace_uuid === activeLease?.workspace_uuid) ?? null,
    [workspaces, activeLease?.workspace_uuid],
  );

  const closeStream = useCallback(() => {
    streamRef.current?.close();
    streamRef.current = null;
  }, []);

  const openAgentStream = useCallback(
    async (agent: AgentSummary, access: WorkspaceAccess) => {
      closeStream();
      setSelectedAgent(agent);
      setConnectionStatus("connecting");
      setError(null);
      const agentAccess = { ...access, agent_uuid: agent.agent_uuid };
      const snapshot = await agentSnapshot(agentAccess);
      setChat({
        ...initialChatState(),
        units: snapshot.units,
        status: snapshot.status,
      });

      const stream = new EventSource(agentEventStreamUrl(agentAccess));
      streamRef.current = stream;
      const consume = (eventName: string) => (event: MessageEvent) => {
        const normalized = normalizeAgentEvent({ eventName, data: event.data });
        if (normalized.type === "connected") {
          setConnectionStatus("connected");
          return;
        }
        setChat((current) => applyAgentEvent(current, normalized));
      };

      for (const eventName of AGENT_EVENT_NAMES) {
        stream.addEventListener(eventName, consume(eventName));
      }
      stream.onerror = () => {
        setConnectionStatus("error");
        setError("Event stream disconnected");
      };
    },
    [closeStream],
  );

  const selectAgent = useCallback(
    async (agent: AgentSummary) => {
      // 找到 agent 所属的 workspace
      const wsUuid = Object.entries(workspaceAgents).find(
        ([, agents]) => agents.some((a) => a.agent_uuid === agent.agent_uuid),
      )?.[0];
      if (!wsUuid) {
        return;
      }
      // 确保该 workspace 的 lease 有效（续租）
      const lease = await ensureWsLease(wsUuid, clientId, workspaceLeases, setWorkspaceLeases);
      if (!lease) {
        return;
      }
      const access: WorkspaceAccess = {
        workspace_uuid: wsUuid,
        client_id: clientId,
        lease_token: lease.lease_token,
      };
      await openAgentStream(agent, access);
    },
    [clientId, openAgentStream, workspaceAgents, workspaceLeases],
  );

  const expandWorkspace = useCallback(
    async (workspace: WorkspaceSummary) => {
      if (expandedWorkspaceUuids.includes(workspace.workspace_uuid)) {
        // 已展开 → 折叠
        setExpandedWorkspaceUuids((prev) => prev.filter((id) => id !== workspace.workspace_uuid));
        setWorkspaceAgents((prev) => {
          const next = { ...prev };
          delete next[workspace.workspace_uuid];
          return next;
        });
        // 如果当前选中的 agent 属于这个 workspace，取消选中
        if (selectedAgent && workspaceAgents[workspace.workspace_uuid]?.some(
          (a) => a.agent_uuid === selectedAgent.agent_uuid,
        )) {
          closeStream();
          setSelectedAgent(null);
          setChat(initialChatState());
        }
        return;
      }

      // 展开：acquire lease（不影响 activeLease）
      const lease = await ensureWsLease(workspace.workspace_uuid, clientId, workspaceLeases, setWorkspaceLeases);
      if (!lease) {
        return;
      }
      setExpandedWorkspaceUuids((prev) => [...prev, workspace.workspace_uuid]);
      setError(null);

      // list agents
      const access: WorkspaceAccess = {
        workspace_uuid: workspace.workspace_uuid,
        client_id: clientId,
        lease_token: lease.lease_token,
      };
      const agents = await listAgents(access);
      setWorkspaceAgents((prev) => ({ ...prev, [workspace.workspace_uuid]: agents }));
    },
    [clientId, closeStream, expandedWorkspaceUuids, selectedAgent, workspaceAgents, workspaceLeases],
  );

  const loadInitialData = useCallback(async () => {
    setError(null);
    const [workspaceList, profileList] = await Promise.all([
      listWorkspaces(),
      listProfiles(),
    ]);
    setWorkspaces(workspaceList);
    setProfiles(profileList);
  }, []);

  const addWorkspace = useCallback(
    async (path: string) => {
      const workspace = await addWorkspaceApi(path);
      const nextWorkspaces = await listWorkspaces();
      setWorkspaces(nextWorkspaces);
      await expandWorkspace(workspace);
    },
    [expandWorkspace],
  );

  const createAgent = useCallback(
    async (wsUuid: string, input: AgentCreateInput) => {
      // 确保目标 workspace 的 lease 有效
      const lease = await ensureWsLease(wsUuid, clientId, workspaceLeases, setWorkspaceLeases);
      if (!lease) {
        return;
      }
      const access: WorkspaceAccess = {
        workspace_uuid: wsUuid,
        client_id: clientId,
        lease_token: lease.lease_token,
      };
      const createdAgent = await createAgentApi(access, input);
      // 刷新该 workspace 的 agent 列表
      const updatedAgents = await listAgents(access);
      setWorkspaceAgents((prev) => ({ ...prev, [wsUuid]: updatedAgents }));
      const created = updatedAgents.find((a) => a.agent_uuid === createdAgent.uuid);
      if (created) {
        await openAgentStream(created, access);
      }
    },
    [clientId, openAgentStream, workspaceLeases],
  );

  const send = useCallback(
    async (text: string) => {
      if (!selectedAgent || !activeLease) {
        return;
      }
      // 发送前续租
      const lease = await ensureWsLease(activeLease.workspace_uuid, clientId, workspaceLeases, setWorkspaceLeases);
      if (!lease) {
        return;
      }
      const access: WorkspaceAccess = {
        workspace_uuid: lease.workspace_uuid,
        client_id: clientId,
        lease_token: lease.lease_token,
      };
      await sendMessage({ ...access, agent_uuid: selectedAgent.agent_uuid }, text);
    },
    [clientId, selectedAgent, activeLease, workspaceLeases],
  );

  const cancel = useCallback(async () => {
    if (!selectedAgent || !activeLease) {
      return;
    }
    const lease = await ensureWsLease(activeLease.workspace_uuid, clientId, workspaceLeases, setWorkspaceLeases);
    if (!lease) {
      return;
    }
    const access: AgentAccess = {
      workspace_uuid: lease.workspace_uuid,
      client_id: clientId,
      lease_token: lease.lease_token,
      agent_uuid: selectedAgent.agent_uuid,
    };
    await cancelAgent(access);
  }, [clientId, selectedAgent, activeLease, workspaceLeases]);

  const approve = useCallback(
    async (approvalMask: number) => {
      if (!selectedAgent || !chat.pendingApproval || !activeLease) {
        return;
      }
      const lease = await ensureWsLease(activeLease.workspace_uuid, clientId, workspaceLeases, setWorkspaceLeases);
      if (!lease) {
        return;
      }
      const access: AgentAccess = {
        workspace_uuid: lease.workspace_uuid,
        client_id: clientId,
        lease_token: lease.lease_token,
        agent_uuid: selectedAgent.agent_uuid,
      };
      await approveRequest(access, chat.pendingApproval.request_uuid, approvalMask);
      setChat((current) => ({ ...current, pendingApproval: null }));
    },
    [chat.pendingApproval, clientId, selectedAgent, activeLease, workspaceLeases],
  );

  useEffect(() => closeStream, [closeStream]);

  return {
    clientId,
    workspaces,
    profiles,
    expandedWorkspaceUuids,
    workspaceLeases,
    workspaceAgents,
    selectedAgent,
    lease: activeLease,
    selectedWorkspace,
    units: chat.units,
    streamingText: chat.streamingText,
    pendingApproval: chat.pendingApproval,
    statusLabel: chat.status ?? selectedAgent?.status ?? "idle",
    connectionStatus,
    error: error ?? chat.errors.at(-1) ?? null,
    loadInitialData,
    expandWorkspace,
    selectAgent,
    addWorkspace,
    createAgent,
    send,
    cancel,
    approve,
  };
}
