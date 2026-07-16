import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ApiError,
  addWorkspace as addWorkspaceApi,
  acquireLease,
  agentSnapshot,
  approveRequest,
  cancelAgent,
  createAgent as createAgentApi,
  deleteAgent as deleteAgentApi,
  deleteWorkspace as deleteWorkspaceApi,
  listAgents,
  listProfiles,
  listWorkspaces,
  releaseLease,
  sendMessage,
} from "../api/client";
import { createWebSocket, parseWsMessage, wsSend } from "../api/events";
import type {
  AgentCreateInput,
  AgentEvent,
  AgentSummary,
  PendingApproval,
  Unit,
  WsClientMessage,
  WsServerMessage,
  WorkspaceLease,
  WorkspaceSummary,
} from "../api/types";
import { applyAgentEvent, initialChatState } from "./sessionModel";

const PENDING_UUID_PREFIX = "__pending-";
const LEASE_RENEW_SKEW_SECONDS = 3;
const LEASE_CONFLICT_MESSAGE =
  "This workspace is currently in use by another client. Try again shortly.";

function createClientId() {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `client-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function upsertAgent(agents: AgentSummary[], agent: AgentSummary) {
  const next = agents.filter((item) => item.agent_uuid !== agent.agent_uuid);
  next.push(agent);
  next.sort((left, right) => left.agent_name.localeCompare(right.agent_name));
  return next;
}

function userFacingError(error: unknown): string {
  if (error instanceof ApiError && error.code === "workspace_lease_conflict") {
    return LEASE_CONFLICT_MESSAGE;
  }
  return error instanceof Error ? error.message : String(error);
}

export type PrismSession = {
  clientId: string;
  workspaces: WorkspaceSummary[];
  profiles: string[];
  expandedWorkspaceUuids: string[];
  workspaceAgents: Record<string, AgentSummary[]>;
  selectedAgent: AgentSummary | null;
  selectedWorkspace: WorkspaceSummary | null;
  session: { workspace_uuid: string } | null;
  units: Unit[];
  streamingText: string;
  streamingReasoningText: string;
  pendingApproval: PendingApproval | null;
  statusLabel: string;
  connectionStatus: "idle" | "connecting" | "connected" | "error";
  error: string | null;
  loadInitialData: () => Promise<void>;
  expandWorkspace: (workspace: WorkspaceSummary) => Promise<void>;
  selectAgent: (agent: AgentSummary) => Promise<void>;
  addWorkspace: (path: string) => Promise<void>;
  createAgent: (workspaceUuid: string, input: AgentCreateInput) => Promise<void>;
  deleteAgent: (agent: AgentSummary) => Promise<void>;
  deleteWorkspace: (workspace: WorkspaceSummary) => Promise<void>;
  send: (text: string) => Promise<void>;
  cancel: () => Promise<void>;
  approve: (approvalMask: number) => Promise<void>;
};

export function usePrismSession(): PrismSession {
  const [clientId] = useState(createClientId);
  const [workspaces, setWorkspaces] = useState<WorkspaceSummary[]>([]);
  const [profiles, setProfiles] = useState<string[]>([]);
  const [expandedWorkspaceUuids, setExpandedWorkspaceUuids] = useState<string[]>([]);
  const [workspaceAgents, setWorkspaceAgents] = useState<Record<string, AgentSummary[]>>({});
  const [workspaceLeases, setWorkspaceLeases] = useState<Record<string, WorkspaceLease & { expires_at: number }>>({});
  const [selectedAgent, setSelectedAgent] = useState<AgentSummary | null>(null);
  const [chat, setChat] = useState(initialChatState);
  const [connectionStatus, setConnectionStatus] =
    useState<PrismSession["connectionStatus"]>("idle");
  const [error, setError] = useState<string | null>(null);

  // --- Refs ---
  const wsRef = useRef<WebSocket | null>(null);
  const wsReadyRef = useRef(false);
  const pendingMessagesRef = useRef<WsClientMessage[]>([]);
  const ignoreStreamUntilNextStatusRef = useRef(false);
  const selectedAgentUuidRef = useRef<string | null>(null);
  const subscribedWorkspaceUuidRef = useRef<string | null>(null);
  const subscribedAgentUuidRef = useRef<string | null>(null);
  const workspaceLeasesRef = useRef(workspaceLeases);

  // Message handler ref — updated every render to capture current state
  const handleWsMessageRef = useRef<(msg: WsServerMessage) => void>(() => {});

  useEffect(() => {
    selectedAgentUuidRef.current = selectedAgent?.agent_uuid ?? null;
  }, [selectedAgent?.agent_uuid]);

  useEffect(() => {
    workspaceLeasesRef.current = workspaceLeases;
  }, [workspaceLeases]);

  // Public REST actions cross this boundary so rejected requests consistently
  // reach the UI. Lower-level helpers remain transport/data-only functions.
  const runRestAction = useCallback(async <T>(action: () => Promise<T>): Promise<T> => {
    setError(null);
    try {
      return await action();
    } catch (error) {
      setError(userFacingError(error));
      throw error;
    }
  }, []);

  // --- Derived state ---

  const activeSession = useMemo(() => {
    if (!selectedAgent) {
      return null;
    }
    const wsUuid = Object.entries(workspaceAgents).find(([, agents]) =>
      agents.some((agent) => agent.agent_uuid === selectedAgent.agent_uuid),
    )?.[0];
    return wsUuid ? { workspace_uuid: wsUuid } : null;
  }, [selectedAgent, workspaceAgents]);

  const selectedWorkspace = useMemo(
    () =>
      workspaces.find((ws) => ws.workspace_uuid === activeSession?.workspace_uuid) ?? null,
    [workspaces, activeSession?.workspace_uuid],
  );

  // --- WebSocket connection management ---

  // Stable sendOrQueue (ensureWs is declared first so closure captures it)
  const ensureWs = useCallback(() => {
    if (
      wsRef.current &&
      (wsRef.current.readyState === WebSocket.OPEN ||
        wsRef.current.readyState === WebSocket.CONNECTING)
    ) {
      return wsRef.current;
    }

    const ws = createWebSocket();
    wsRef.current = ws;

    ws.onopen = () => {
      wsReadyRef.current = true;
      // Flush queued messages
      for (const msg of pendingMessagesRef.current) {
        wsSend(ws, msg);
      }
      pendingMessagesRef.current = [];
      // Re-subscribe on reconnect
      if (subscribedWorkspaceUuidRef.current) {
        wsSend(ws, {
          type: "subscribe_workspace",
          workspace_uuid: subscribedWorkspaceUuidRef.current,
        });
      }
      if (subscribedAgentUuidRef.current) {
        wsSend(ws, {
          type: "subscribe_agent",
          agent_uuid: subscribedAgentUuidRef.current,
        });
      }
    };

    ws.onmessage = (event) => {
      handleWsMessageRef.current(parseWsMessage(event.data as string));
    };

    ws.onclose = () => {
      wsReadyRef.current = false;
      wsRef.current = null;
      // Auto-reconnect after 1 second
      setTimeout(() => ensureWs(), 1000);
    };

    return ws;
  }, []);

  const sendOrQueue = useCallback(
    (msg: WsClientMessage) => {
      const ws = wsRef.current;
      if (ws?.readyState === WebSocket.OPEN) {
        wsSend(ws, msg);
      } else {
        pendingMessagesRef.current.push(msg);
        ensureWs();
      }
    },
    [ensureWs],
  );

  // ---- Unified workspace removal ----
  // Called by deleteWorkspace() after API success, and by workspace_deleted WS event.
  const removeWorkspaceLocally = useCallback((workspaceUuid: string) => {
    // Unsubscribe via WS if this workspace is currently subscribed
    if (subscribedWorkspaceUuidRef.current === workspaceUuid) {
      sendOrQueue({ type: "unsubscribe_workspace" });
      if (subscribedAgentUuidRef.current) {
        sendOrQueue({ type: "unsubscribe_agent" });
        subscribedAgentUuidRef.current = null;
      }
      subscribedWorkspaceUuidRef.current = null;
      selectedAgentUuidRef.current = null;
      setChat(initialChatState());
      setConnectionStatus("idle");
    }

    // Clear selected agent if it belongs to this workspace
    setSelectedAgent((current) => {
      if (!current) return null;
      return workspaceAgents[workspaceUuid]?.some(
        (a) => a.agent_uuid === current.agent_uuid,
      )
        ? null
        : current;
    });

    // Remove workspace from all state collections
    setWorkspaces((prev) => prev.filter((ws) => ws.workspace_uuid !== workspaceUuid));
    setExpandedWorkspaceUuids((prev) => prev.filter((id) => id !== workspaceUuid));
    setWorkspaceAgents((prev) => {
      const next = { ...prev };
      delete next[workspaceUuid];
      return next;
    });
    setWorkspaceLeases((prev) => {
      const next = { ...prev };
      delete next[workspaceUuid];
      return next;
    });
  }, [sendOrQueue, workspaceAgents]);

  const ensureWorkspaceLease = useCallback(
    async (workspaceUuid: string): Promise<WorkspaceLease> => {
      const existing = workspaceLeases[workspaceUuid];
      const now = Math.floor(Date.now() / 1000);
      if (existing && existing.expires_at - now > LEASE_RENEW_SKEW_SECONDS) {
        return {
          workspace_uuid: existing.workspace_uuid,
          lease_token: existing.lease_token,
        };
      }

      const lease = await acquireLease(
        workspaceUuid,
        clientId,
        existing?.lease_token ?? null,
      );
      const access = {
        workspace_uuid: lease.workspace_uuid,
        lease_token: lease.lease_token,
        expires_at: lease.expires_at,
      };
      setWorkspaceLeases((prev) => ({
        ...prev,
        [workspaceUuid]: access,
      }));
      setWorkspaces((prev) =>
        prev.map((workspace) =>
          workspace.workspace_uuid === workspaceUuid
            ? { ...workspace, locked_by: lease.client_id }
            : workspace,
        ),
      );
      return {
        workspace_uuid: access.workspace_uuid,
        lease_token: access.lease_token,
      };
    },
    [clientId, workspaceLeases],
  );

  // Update message handler on each render
  handleWsMessageRef.current = (msg: WsServerMessage) => {
    // --- Ping / Connected / Error ---
    if (msg.type === "ping") {
      sendOrQueue({ type: "pong" });
      return;
    }
    if (msg.type === "connected") {
      setConnectionStatus("connected");
      return;
    }
    if (msg.type === "error") {
      setError(msg.message);
      return;
    }

    // --- Workspace events ---
    const wsUuid = subscribedWorkspaceUuidRef.current;

    if (msg.type === "agent_created" || msg.type === "agent_updated") {
      if (wsUuid) {
        setWorkspaceAgents((prev) => ({
          ...prev,
          [wsUuid]: upsertAgent(prev[wsUuid] ?? [], msg.agent),
        }));
        setSelectedAgent((current) =>
          current?.agent_uuid === msg.agent.agent_uuid ? msg.agent : current,
        );
      }
      return;
    }

    if (msg.type === "agent_status_changed") {
      if (wsUuid) {
        setWorkspaceAgents((prev) => ({
          ...prev,
          [wsUuid]: (prev[wsUuid] ?? []).map((agent) =>
            agent.agent_uuid === msg.agent_uuid
              ? { ...agent, status: msg.status }
              : agent,
          ),
        }));
        setSelectedAgent((current) =>
          current?.agent_uuid === msg.agent_uuid
            ? { ...current, status: msg.status }
            : current,
        );
      }
      return;
    }

    if (msg.type === "agent_deleted") {
      if (wsUuid) {
        setWorkspaceAgents((prev) => ({
          ...prev,
          [wsUuid]: (prev[wsUuid] ?? []).filter(
            (agent) => agent.agent_uuid !== msg.agent_uuid,
          ),
        }));
        setSelectedAgent((current) =>
          current?.agent_uuid === msg.agent_uuid ? null : current,
        );
        if (selectedAgentUuidRef.current === msg.agent_uuid) {
          subscribedAgentUuidRef.current = null;
          setChat(initialChatState());
          setConnectionStatus("idle");
        }
      }
      return;
    }

    if (msg.type === "workspace_deleted") {
      removeWorkspaceLocally(msg.workspace_uuid);
      return;
    }

    // Workspace resource events — not yet handled in UI, safely ignore
    if (
      msg.type === "context_created" ||
      msg.type === "workflow_created" ||
      msg.type === "workflow_started" ||
      msg.type === "workflow_cancel_requested"
    ) {
      return;
    }

    // --- Agent events ---
    if (
      (msg.type === "stream_delta" || msg.type === "reasoning_delta") &&
      ignoreStreamUntilNextStatusRef.current
    ) {
      return;
    }
    if (msg.type === "status_changed") {
      ignoreStreamUntilNextStatusRef.current = false;
    }
    if (
      msg.type === "operation_failed" &&
      msg.agent_uuid !== selectedAgentUuidRef.current
    ) {
      return;
    }
    setChat((current) => applyAgentEvent(current, msg as AgentEvent));
  };

  // --- Actions ---

  const expandWorkspace = useCallback(
    async (workspace: WorkspaceSummary) => {
      if (expandedWorkspaceUuids.includes(workspace.workspace_uuid)) {
        // Collapse: unsubscribe agent if it belongs to this workspace
        if (
          selectedAgent &&
          workspaceAgents[workspace.workspace_uuid]?.some(
            (agent) => agent.agent_uuid === selectedAgent.agent_uuid,
          )
        ) {
          sendOrQueue({ type: "unsubscribe_agent" });
          subscribedAgentUuidRef.current = null;
          setSelectedAgent(null);
          setChat(initialChatState());
          setConnectionStatus("idle");
        }
        // Unsubscribe workspace
        sendOrQueue({ type: "unsubscribe_workspace" });
        subscribedWorkspaceUuidRef.current = null;
        setExpandedWorkspaceUuids((prev) =>
          prev.filter((id) => id !== workspace.workspace_uuid),
        );
        setWorkspaceAgents((prev) => {
          const next = { ...prev };
          delete next[workspace.workspace_uuid];
          return next;
        });
        return;
      }

      // Expand: subscribe workspace and fetch agents
      sendOrQueue({ type: "subscribe_workspace", workspace_uuid: workspace.workspace_uuid });
      subscribedWorkspaceUuidRef.current = workspace.workspace_uuid;
      setExpandedWorkspaceUuids((prev) => [...prev, workspace.workspace_uuid]);
      setError(null);

      await runRestAction(async () => {
        const agents = await listAgents(workspace.workspace_uuid);
        setWorkspaceAgents((prev) => ({ ...prev, [workspace.workspace_uuid]: agents }));
      });
    },
    [expandedWorkspaceUuids, runRestAction, selectedAgent, sendOrQueue, workspaceAgents],
  );

  const selectAgent = useCallback(
    async (agent: AgentSummary) => {
      const wsUuid = Object.entries(workspaceAgents).find(([, agents]) =>
        agents.some((candidate) => candidate.agent_uuid === agent.agent_uuid),
      )?.[0];
      if (!wsUuid) {
        return;
      }

      // If the workspace changed, switch subscriptions
      if (subscribedWorkspaceUuidRef.current && subscribedWorkspaceUuidRef.current !== wsUuid) {
        sendOrQueue({ type: "unsubscribe_workspace" });
        if (subscribedAgentUuidRef.current) {
          sendOrQueue({ type: "unsubscribe_agent" });
          subscribedAgentUuidRef.current = null;
        }
      }
      if (subscribedWorkspaceUuidRef.current !== wsUuid) {
        sendOrQueue({ type: "subscribe_workspace", workspace_uuid: wsUuid });
        subscribedWorkspaceUuidRef.current = wsUuid;
      }

      // Subscribe to agent (server auto-unsubscribes previous)
      sendOrQueue({ type: "subscribe_agent", agent_uuid: agent.agent_uuid });
      subscribedAgentUuidRef.current = agent.agent_uuid;
      selectedAgentUuidRef.current = agent.agent_uuid;
      setSelectedAgent(agent);
      setChat(initialChatState());
      setConnectionStatus("connecting");
      setError(null);
      ignoreStreamUntilNextStatusRef.current = false;

      // Fetch snapshot via REST
      const snapshot = await runRestAction(() => agentSnapshot(wsUuid, agent.agent_uuid));
      if (selectedAgentUuidRef.current !== agent.agent_uuid) {
        return;
      }
      setChat({
        ...initialChatState(),
        units: snapshot.units,
        status: snapshot.status,
        pendingApproval: snapshot.pending_approval,
      });
    },
    [runRestAction, sendOrQueue, workspaceAgents],
  );

  const loadInitialData = useCallback(
    () => runRestAction(async () => {
      const [workspaceList, profileList] = await Promise.all([
        listWorkspaces(),
        listProfiles(),
      ]);
      setWorkspaces(workspaceList);
      setProfiles(profileList);
    }),
    [runRestAction],
  );

  const addWorkspace = useCallback(
    async (path: string) => {
      const workspace = await runRestAction(() => addWorkspaceApi(path));
      const nextWorkspaces = await runRestAction(listWorkspaces);
      setWorkspaces(nextWorkspaces);
      await expandWorkspace(workspace);
    },
    [expandWorkspace, runRestAction],
  );

  const createAgent = useCallback(
    async (wsUuid: string, input: AgentCreateInput) => {
      await runRestAction(async () => {
        const access = await ensureWorkspaceLease(wsUuid);
        await createAgentApi(access, input);
      });
    },
    [ensureWorkspaceLease, runRestAction],
  );

  const deleteAgent = useCallback(
    async (agent: AgentSummary) => {
      const wsUuid = Object.entries(workspaceAgents).find(([, agents]) =>
        agents.some((candidate) => candidate.agent_uuid === agent.agent_uuid),
      )?.[0];
      if (!wsUuid) {
        return;
      }
      await runRestAction(async () => {
        const access = await ensureWorkspaceLease(wsUuid);
        await deleteAgentApi(access, agent.agent_uuid);
      });
    },
    [ensureWorkspaceLease, runRestAction, workspaceAgents],
  );

  const deleteWorkspace = useCallback(
    async (workspace: WorkspaceSummary) => {
      await runRestAction(async () => {
        const access = await ensureWorkspaceLease(workspace.workspace_uuid);
        await deleteWorkspaceApi(access);
      });
      removeWorkspaceLocally(workspace.workspace_uuid);
    },
    [ensureWorkspaceLease, removeWorkspaceLocally, runRestAction],
  );

  const send = useCallback(
    async (text: string) => {
      if (!selectedAgent || !activeSession) {
        throw new Error("No agent selected");
      }

      const optimisticUuid = `${PENDING_UUID_PREFIX}${Date.now()}-${Math.random()
        .toString(36)
        .slice(2, 8)}`;
      const optimisticUnit: Unit = {
        uuid: optimisticUuid,
        visibility: "public",
        content: { role: "user", content: text },
        token_usage: null,
        metadata: {},
        created_at: Math.floor(Date.now() / 1000),
      };
      setChat((current) => ({
        ...current,
        units: [...current.units, optimisticUnit],
        pendingUserUuid: optimisticUuid,
      }));

      try {
        await runRestAction(async () => {
          const access = await ensureWorkspaceLease(activeSession.workspace_uuid);
          await sendMessage(
            access,
            selectedAgent.agent_uuid,
            text,
          );
        });
      } catch (err) {
        setChat((current) => ({
          ...current,
          units: current.units.filter((unit) => unit.uuid !== optimisticUuid),
          pendingUserUuid:
            current.pendingUserUuid === optimisticUuid ? null : current.pendingUserUuid,
        }));
        throw err;
      }
    },
    [activeSession, ensureWorkspaceLease, runRestAction, selectedAgent],
  );

  const approve = useCallback(
    async (approvalMask: number) => {
      if (!selectedAgent || !chat.pendingApproval || !activeSession) {
        return;
      }
      const requestUuid = chat.pendingApproval.request_uuid;
      await runRestAction(async () => {
        const access = await ensureWorkspaceLease(activeSession.workspace_uuid);
        await approveRequest(
          access,
          selectedAgent.agent_uuid,
          requestUuid,
          approvalMask,
        );
      });
      setChat((current) => ({
        ...current,
        pendingApproval: null,
        streamingText: "",
        streamingReasoningText: "",
      }));
    },
    [activeSession, chat.pendingApproval, ensureWorkspaceLease, runRestAction, selectedAgent],
  );

  const cancel = useCallback(async () => {
    if (!selectedAgent || !activeSession) {
      return;
    }
    const status = chat.status ?? selectedAgent.status;
    if (status === "waiting_approval") {
      await approve(0);
      return;
    }

    if (status === "running_llm") {
      ignoreStreamUntilNextStatusRef.current = true;
      setChat((current) => ({
        ...current,
        units: current.pendingUserUuid
          ? current.units.filter((unit) => unit.uuid !== current.pendingUserUuid)
          : current.units,
        pendingUserUuid: null,
        streamingText: "",
        streamingReasoningText: "",
      }));
      try {
        await runRestAction(async () => {
          const access = await ensureWorkspaceLease(activeSession.workspace_uuid);
          await cancelAgent(access, selectedAgent.agent_uuid);
        });
      } catch (err) {
        ignoreStreamUntilNextStatusRef.current = false;
        throw err;
      }
      return;
    }

    setChat((current) => ({
      ...current,
      streamingText: "",
      streamingReasoningText: "",
    }));
    await runRestAction(async () => {
      const access = await ensureWorkspaceLease(activeSession.workspace_uuid);
      await cancelAgent(access, selectedAgent.agent_uuid);
    });
  }, [activeSession, approve, chat.status, ensureWorkspaceLease, runRestAction, selectedAgent]);

  // Cleanup on unmount
  useEffect(
    () => () => {
      wsRef.current?.close();
      wsRef.current = null;
      for (const lease of Object.values(workspaceLeasesRef.current)) {
        void releaseLease(lease.workspace_uuid, lease.lease_token).catch(() => {});
      }
    },
    [],
  );

  return {
    clientId,
    workspaces,
    profiles,
    expandedWorkspaceUuids,
    workspaceAgents,
    selectedAgent,
    session: activeSession,
    selectedWorkspace,
    units: chat.units,
    streamingText: chat.streamingText,
    streamingReasoningText: chat.streamingReasoningText,
    pendingApproval: chat.pendingApproval,
    statusLabel: chat.status ?? selectedAgent?.status ?? "idle",
    connectionStatus,
    error: error ?? chat.errors.at(-1) ?? null,
    loadInitialData,
    expandWorkspace,
    selectAgent,
    addWorkspace,
    createAgent,
    deleteAgent,
    deleteWorkspace,
    send,
    cancel,
    approve,
  };
}
