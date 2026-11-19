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
import { watchSubscription, type SubscriptionEvent } from "../api/events";
import { sameAgent } from "../api/types";
import type {
  AgentAddr,
  AgentCreateInput,
  AgentSummary,
  PendingApproval,
  Unit,
  WorkspaceLease,
  WorkspaceSummary,
} from "../api/types";
import { applyAgentEvent, initialChatState } from "./sessionModel";
import { createStreamDeltaBuffer } from "./streamDeltaBuffer";

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
  workspaceErrors: Record<string, string>;
  selectedAgent: AgentSummary | null;
  selectedWorkspace: WorkspaceSummary | null;
  selectedAddr: AgentAddr | null;
  units: Unit[];
  streamingText: string;
  streamingReasoningText: string;
  pendingApproval: PendingApproval | null;
  statusLabel: string;
  connectionStatus: "idle" | "connecting" | "connected" | "error";
  error: string | null;
  loadInitialData: () => Promise<void>;
  expandWorkspace: (workspace: WorkspaceSummary) => Promise<void>;
  selectAgent: (addr: AgentAddr) => Promise<void>;
  addWorkspace: (path: string) => Promise<void>;
  createAgent: (workspaceUuid: string, input: AgentCreateInput) => Promise<void>;
  deleteAgent: (addr: AgentAddr) => Promise<void>;
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
  const [workspaceErrors, setWorkspaceErrors] = useState<Record<string, string>>({});
  const [selectedAddr, setSelectedAddr] = useState<AgentAddr | null>(null);
  const [chat, setChat] = useState(initialChatState);
  const [connectionStatus, setConnectionStatus] =
    useState<PrismSession["connectionStatus"]>("idle");
  const [error, setError] = useState<string | null>(null);

  // --- Refs ---
  const ignoreStreamUntilNextStatusRef = useRef(false);
  const selectionRef = useRef<AgentAddr | null>(null);
  const workspaceLeasesRef = useRef(workspaceLeases);
  const streamDeltaBufferRef = useRef<ReturnType<
    typeof createStreamDeltaBuffer
  > | null>(null);
  if (!streamDeltaBufferRef.current) {
    streamDeltaBufferRef.current = createStreamDeltaBuffer(
      ({ text, reasoningText }) => {
        setChat((current) => ({
          ...current,
          streamingText: current.streamingText + text,
          streamingReasoningText:
            current.streamingReasoningText + reasoningText,
        }));
      },
    );
  }
  const streamDeltaBuffer = streamDeltaBufferRef.current;

  useEffect(() => {
    workspaceLeasesRef.current = workspaceLeases;
  }, [workspaceLeases]);

  const reportWorkspaceError = useCallback((id: string, message: string | null) => {
    setWorkspaceErrors(prev => {
      const next = { ...prev };
      if (message === null) delete next[id];
      else next[id] = message;
      return next;
    });
  }, []);

  // Public REST actions cross this boundary so rejected requests consistently
  // reach the UI. Lower-level helpers remain transport/data-only functions.
  const runRestAction = useCallback(async <T>(action: () => Promise<T>, owner?: AgentAddr | string): Promise<T> => {
    if (typeof owner === "string") reportWorkspaceError(owner, null);
    else setError(null);
    try {
      return await action();
    } catch (error) {
      if (typeof owner === "string") reportWorkspaceError(owner, userFacingError(error));
      else if (!owner || owner === selectionRef.current) setError(userFacingError(error));
      throw error;
    }
  }, [reportWorkspaceError]);

  // --- Derived state ---

  const selectedAgent = selectedAddr
    ? workspaceAgents[selectedAddr.workspace_uuid]?.find(agent => agent.agent_uuid === selectedAddr.agent_uuid) ?? null
    : null;
  const selectedWorkspace = useMemo(
    () => workspaces.find(ws => ws.workspace_uuid === selectedAddr?.workspace_uuid) ?? null,
    [workspaces, selectedAddr],
  );

  const clearSelection = useCallback(() => {
    selectionRef.current = null;
    setSelectedAddr(null);
    streamDeltaBuffer.clear();
    setChat(initialChatState());
    setConnectionStatus("idle");
    setError(null);
  }, [streamDeltaBuffer]);

  const removeAgentLocally = useCallback((addr: AgentAddr) => {
    if (sameAgent(selectionRef.current, addr)) clearSelection();
    setWorkspaceAgents(prev => ({
      ...prev,
      [addr.workspace_uuid]: (prev[addr.workspace_uuid] ?? []).filter(agent => agent.agent_uuid !== addr.agent_uuid),
    }));
  }, [clearSelection]);

  const removeWorkspaceLocally = useCallback((workspaceUuid: string) => {
    if (selectionRef.current?.workspace_uuid === workspaceUuid) clearSelection();
    setWorkspaces(prev => prev.filter(ws => ws.workspace_uuid !== workspaceUuid));
    setExpandedWorkspaceUuids(prev => prev.filter(id => id !== workspaceUuid));
    setWorkspaceAgents(prev => {
      const next = { ...prev };
      delete next[workspaceUuid];
      return next;
    });
    setWorkspaceLeases(prev => {
      const next = { ...prev };
      delete next[workspaceUuid];
      return next;
    });
  }, [clearSelection]);

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

  const handleWorkspaceEvent = useCallback((wsUuid: string, msg: SubscriptionEvent) => {
    if (msg.type === "agent_created" || msg.type === "agent_updated") {
      setWorkspaceAgents((prev) => ({
        ...prev,
        [wsUuid]: upsertAgent(prev[wsUuid] ?? [], msg.agent),
      }));
      return;
    }

    if (msg.type === "agent_status_changed") {
      setWorkspaceAgents((prev) => ({
        ...prev,
        [wsUuid]: (prev[wsUuid] ?? []).map((agent) =>
          agent.agent_uuid === msg.agent_uuid
            ? { ...agent, status: msg.status }
            : agent,
        ),
      }));
      return;
    }

    if (msg.type === "agent_deleted") {
      removeAgentLocally({ workspace_uuid: wsUuid, agent_uuid: msg.agent_uuid });
      return;
    }

    if (msg.type === "workspace_deleted") {
      removeWorkspaceLocally(msg.workspace_uuid);
    }
  }, [removeAgentLocally, removeWorkspaceLocally]);

  const sidebarSubscriptions = useRef(new Map<string, () => void>());
  useEffect(() => {
    const subscriptions = sidebarSubscriptions.current;
    for (const [id, close] of subscriptions) {
      if (expandedWorkspaceUuids.includes(id)) continue;
      close();
      subscriptions.delete(id);
      reportWorkspaceError(id, null);
    }
    for (const id of expandedWorkspaceUuids) {
      if (subscriptions.has(id)) continue;
      subscriptions.set(id, watchSubscription(id, {
        load: async () => {
          const agents = await listAgents(id);
          return () => {
            setWorkspaceAgents(prev => ({ ...prev, [id]: agents }));
            const selected = selectionRef.current;
            if (selected?.workspace_uuid === id && !agents.some(agent => agent.agent_uuid === selected.agent_uuid)) {
              clearSelection();
            }
            reportWorkspaceError(id, null);
          };
        },
        receive: msg => handleWorkspaceEvent(id, msg),
        disconnected: () => reportWorkspaceError(id, "Workspace disconnected. Reconnecting..."),
        failed: error => reportWorkspaceError(id, userFacingError(error)),
      }));
    }
  }, [expandedWorkspaceUuids, clearSelection, handleWorkspaceEvent, reportWorkspaceError]);
  useEffect(() => {
    const subscriptions = sidebarSubscriptions.current;
    return () => {
      for (const close of subscriptions.values()) close();
      subscriptions.clear();
    };
  }, []);

  useEffect(() => {
    const addr = selectedAddr;
    if (!addr) return;
    return watchSubscription(addr, {
      load: async () => {
        const snapshot = await agentSnapshot(addr);
        return () => {
          if (selectionRef.current !== addr) return;
          streamDeltaBuffer.clear();
          ignoreStreamUntilNextStatusRef.current = false;
          setChat({
            ...initialChatState(),
            units: snapshot.units,
            status: snapshot.status,
            pendingApproval: snapshot.pending_approval,
          });
          setConnectionStatus("connected");
          setError(null);
        };
      },
      disconnected: () => {
        if (selectionRef.current === addr) setConnectionStatus("connecting");
      },
      failed: error => {
        if (selectionRef.current !== addr) return;
        setConnectionStatus("error");
        setError(userFacingError(error));
      },
      receive: msg => {
        if (selectionRef.current !== addr) return;
        switch (msg.type) {
          case "stream_delta":
          case "reasoning_delta":
            if (ignoreStreamUntilNextStatusRef.current) return;
            if (msg.type === "stream_delta") streamDeltaBuffer.appendText(msg.text);
            else streamDeltaBuffer.appendReasoning(msg.text);
            return;
          case "status_changed":
            ignoreStreamUntilNextStatusRef.current = false;
            break;
          case "operation_failed":
            if (msg.agent_uuid !== addr.agent_uuid ||
                (msg.workspace_uuid !== null && msg.workspace_uuid !== addr.workspace_uuid)) return;
            break;
          case "approve_request":
          case "unit_append":
            break;
          default:
            return; // Workspace events belong exclusively to the folder subscription.
        }
        if (msg.type === "unit_append" || msg.type === "operation_failed") streamDeltaBuffer.clear();
        else streamDeltaBuffer.flush();
        setChat(current => applyAgentEvent(current, msg));
      },
    });
  }, [selectedAddr, streamDeltaBuffer]);

  // --- Actions ---

  const expandWorkspace = useCallback(
    async (workspace: WorkspaceSummary) => {
      if (expandedWorkspaceUuids.includes(workspace.workspace_uuid)) {
        if (selectionRef.current?.workspace_uuid === workspace.workspace_uuid) clearSelection();
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

      setExpandedWorkspaceUuids((prev) => [...prev, workspace.workspace_uuid]);
    },
    [expandedWorkspaceUuids, clearSelection],
  );

  const selectAgent = useCallback(
    async (target: AgentAddr) => {
      // Each selection owns its requests, including A -> B -> A and retries.
      const addr = { ...target };
      selectionRef.current = addr;
      setSelectedAddr(addr);
      streamDeltaBuffer.clear();
      setChat(initialChatState());
      setConnectionStatus("connecting");
      setError(null);
      ignoreStreamUntilNextStatusRef.current = false;
    },
    [streamDeltaBuffer],
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
      }, wsUuid);
    },
    [ensureWorkspaceLease, runRestAction],
  );

  const deleteAgent = useCallback(
    async (addr: AgentAddr) => {
      await runRestAction(async () => {
        const access = await ensureWorkspaceLease(addr.workspace_uuid);
        await deleteAgentApi(addr, access.lease_token);
      }, addr.workspace_uuid);
      removeAgentLocally(addr);
    },
    [ensureWorkspaceLease, runRestAction, removeAgentLocally],
  );

  const deleteWorkspace = useCallback(
    async (workspace: WorkspaceSummary) => {
      await runRestAction(async () => {
        const access = await ensureWorkspaceLease(workspace.workspace_uuid);
        await deleteWorkspaceApi(access);
      }, workspace.workspace_uuid);
      removeWorkspaceLocally(workspace.workspace_uuid);
    },
    [ensureWorkspaceLease, removeWorkspaceLocally, runRestAction],
  );

  const send = useCallback(
    async (text: string) => {
      if (!selectedAgent || !selectedAddr) {
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
          const access = await ensureWorkspaceLease(selectedAddr.workspace_uuid);
          await sendMessage(
            selectedAddr,
            access.lease_token,
            text,
          );
        }, selectedAddr);
      } catch (err) {
        if (selectionRef.current !== selectedAddr) throw err;
        setChat((current) => ({
          ...current,
          units: current.units.filter((unit) => unit.uuid !== optimisticUuid),
          pendingUserUuid:
            current.pendingUserUuid === optimisticUuid ? null : current.pendingUserUuid,
        }));
        throw err;
      }
    },
    [selectedAddr, ensureWorkspaceLease, runRestAction, selectedAgent],
  );

  const approve = useCallback(
    async (approvalMask: number) => {
      if (!selectedAgent || !chat.pendingApproval || !selectedAddr) {
        return;
      }
      const requestUuid = chat.pendingApproval.request_uuid;
      await runRestAction(async () => {
        const access = await ensureWorkspaceLease(selectedAddr.workspace_uuid);
        await approveRequest(
          selectedAddr,
          access.lease_token,
          requestUuid,
          approvalMask,
        );
      }, selectedAddr);
      if (selectionRef.current !== selectedAddr) return;
      streamDeltaBuffer.clear();
      setChat((current) => ({
        ...current,
        pendingApproval: null,
        streamingText: "",
        streamingReasoningText: "",
      }));
    },
    [
      selectedAddr,
      chat.pendingApproval,
      ensureWorkspaceLease,
      runRestAction,
      selectedAgent,
      streamDeltaBuffer,
    ],
  );

  const cancel = useCallback(async () => {
    if (!selectedAgent || !selectedAddr) {
      return;
    }
    const status = chat.status ?? selectedAgent.status;
    if (status === "waiting_approval") {
      await approve(0);
      return;
    }

    if (status === "running_llm") {
      ignoreStreamUntilNextStatusRef.current = true;
      streamDeltaBuffer.clear();
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
          const access = await ensureWorkspaceLease(selectedAddr.workspace_uuid);
          await cancelAgent(selectedAddr, access.lease_token);
        }, selectedAddr);
      } catch (err) {
        if (selectionRef.current === selectedAddr) ignoreStreamUntilNextStatusRef.current = false;
        throw err;
      }
      return;
    }

    streamDeltaBuffer.clear();
    setChat((current) => ({
      ...current,
      streamingText: "",
      streamingReasoningText: "",
    }));
    await runRestAction(async () => {
      const access = await ensureWorkspaceLease(selectedAddr.workspace_uuid);
      await cancelAgent(selectedAddr, access.lease_token);
    }, selectedAddr);
  }, [
    selectedAddr,
    approve,
    chat.status,
    ensureWorkspaceLease,
    runRestAction,
    selectedAgent,
    streamDeltaBuffer,
  ]);

  // Cleanup on unmount
  useEffect(
    () => () => {
      streamDeltaBuffer.clear();
      for (const lease of Object.values(workspaceLeasesRef.current)) {
        void releaseLease(lease.workspace_uuid, lease.lease_token).catch(() => {});
      }
    },
    [streamDeltaBuffer],
  );

  return {
    clientId,
    workspaces,
    profiles,
    expandedWorkspaceUuids,
    workspaceAgents,
    workspaceErrors,
    selectedAgent,
    selectedAddr,
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
