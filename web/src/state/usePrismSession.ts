import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  addWorkspace as addWorkspaceApi,
  agentSnapshot,
  approveRequest,
  cancelAgent,
  createAgent as createAgentApi,
  deleteAgent as deleteAgentApi,
  listAgents,
  listProfiles,
  listWorkspaces,
  sendMessage,
} from "../api/client";
import {
  agentEventStreamUrl,
  normalizeAgentEvent,
  normalizeWorkspaceEvent,
  workspaceEventStreamUrl,
} from "../api/events";
import type {
  AgentAccess,
  AgentCreateInput,
  AgentSummary,
  PendingApproval,
  Unit,
  WorkspaceAccess,
  WorkspaceSession,
  WorkspaceSummary,
} from "../api/types";
import { applyAgentEvent, initialChatState } from "./sessionModel";

const AGENT_EVENT_NAMES = [
  "connected",
  "unit_append",
  "stream_delta",
  "approve_request",
  "status_changed",
  "error",
];

const WORKSPACE_EVENT_NAMES = [
  "connected",
  "agent_created",
  "agent_updated",
  "agent_status_changed",
  "agent_deleted",
  "context_created",
  "workflow_created",
  "workflow_started",
  "workflow_cancel_requested",
  "error",
];

const PENDING_UUID_PREFIX = "__pending-";

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

export type PrismSession = {
  clientId: string;
  workspaces: WorkspaceSummary[];
  profiles: string[];
  expandedWorkspaceUuids: string[];
  workspaceSessions: Record<string, WorkspaceSession>;
  workspaceAgents: Record<string, AgentSummary[]>;
  selectedAgent: AgentSummary | null;
  selectedWorkspace: WorkspaceSummary | null;
  session: WorkspaceSession | null;
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
  deleteAgent: (agent: AgentSummary) => Promise<void>;
  send: (text: string) => Promise<void>;
  cancel: () => Promise<void>;
  approve: (approvalMask: number) => Promise<void>;
};

export function usePrismSession(): PrismSession {
  const [clientId] = useState(createClientId);
  const [workspaces, setWorkspaces] = useState<WorkspaceSummary[]>([]);
  const [profiles, setProfiles] = useState<string[]>([]);
  const [expandedWorkspaceUuids, setExpandedWorkspaceUuids] = useState<string[]>([]);
  const [workspaceSessions, setWorkspaceSessions] = useState<Record<string, WorkspaceSession>>({});
  const [workspaceAgents, setWorkspaceAgents] = useState<Record<string, AgentSummary[]>>({});
  const [selectedAgent, setSelectedAgent] = useState<AgentSummary | null>(null);
  const [chat, setChat] = useState(initialChatState);
  const [connectionStatus, setConnectionStatus] =
    useState<PrismSession["connectionStatus"]>("idle");
  const [error, setError] = useState<string | null>(null);
  const agentStreamRef = useRef<EventSource | null>(null);
  const workspaceStreamsRef = useRef<Record<string, EventSource>>({});
  const ignoreStreamUntilNextStatusRef = useRef(false);
  const selectedAgentUuidRef = useRef<string | null>(null);

  useEffect(() => {
    selectedAgentUuidRef.current = selectedAgent?.agent_uuid ?? null;
  }, [selectedAgent?.agent_uuid]);

  const activeSession = useMemo<WorkspaceSession | null>(() => {
    if (!selectedAgent) {
      return null;
    }
    const wsUuid = Object.entries(workspaceAgents).find(([, agents]) =>
      agents.some((agent) => agent.agent_uuid === selectedAgent.agent_uuid),
    )?.[0];
    if (!wsUuid) {
      return null;
    }
    return workspaceSessions[wsUuid] ?? null;
  }, [selectedAgent, workspaceAgents, workspaceSessions]);

  const selectedWorkspace = useMemo(
    () => workspaces.find((ws) => ws.workspace_uuid === activeSession?.workspace_uuid) ?? null,
    [workspaces, activeSession?.workspace_uuid],
  );

  const closeAgentStream = useCallback(() => {
    agentStreamRef.current?.close();
    agentStreamRef.current = null;
  }, []);

  const closeWorkspaceStream = useCallback((workspaceUuid: string) => {
    workspaceStreamsRef.current[workspaceUuid]?.close();
    delete workspaceStreamsRef.current[workspaceUuid];
    setWorkspaceSessions((prev) => {
      const next = { ...prev };
      delete next[workspaceUuid];
      return next;
    });
  }, []);

  const openWorkspaceStream = useCallback(
    async (workspaceUuid: string): Promise<WorkspaceAccess> => {
      const access: WorkspaceAccess = { workspace_uuid: workspaceUuid, client_id: clientId };
      if (workspaceStreamsRef.current[workspaceUuid]) {
        return access;
      }

      await new Promise<void>((resolve, reject) => {
        let connected = false;
        const stream = new EventSource(workspaceEventStreamUrl(access));
        workspaceStreamsRef.current[workspaceUuid] = stream;

        const consume = (eventName: string) => (event: MessageEvent) => {
          const normalized = normalizeWorkspaceEvent({ eventName, data: event.data });
          if (normalized.type === "connected") {
            connected = true;
            setWorkspaceSessions((prev) => ({
              ...prev,
              [workspaceUuid]: {
                workspace_uuid: workspaceUuid,
                client_id: clientId,
                connected: true,
              },
            }));
            setWorkspaces((prev) =>
              prev.map((workspace) =>
                workspace.workspace_uuid === workspaceUuid
                  ? { ...workspace, locked_by: clientId }
                  : workspace,
              ),
            );
            resolve();
            return;
          }
          if (normalized.type === "agent_created" || normalized.type === "agent_updated") {
            setWorkspaceAgents((prev) => ({
              ...prev,
              [workspaceUuid]: upsertAgent(prev[workspaceUuid] ?? [], normalized.agent),
            }));
            setSelectedAgent((current) =>
              current?.agent_uuid === normalized.agent.agent_uuid ? normalized.agent : current,
            );
            return;
          }
          if (normalized.type === "agent_status_changed") {
            setWorkspaceAgents((prev) => ({
              ...prev,
              [workspaceUuid]: (prev[workspaceUuid] ?? []).map((agent) =>
                agent.agent_uuid === normalized.agent_uuid
                  ? { ...agent, status: normalized.status }
                  : agent,
              ),
            }));
            setSelectedAgent((current) =>
              current?.agent_uuid === normalized.agent_uuid
                ? { ...current, status: normalized.status }
                : current,
            );
            return;
          }
          if (normalized.type === "agent_deleted") {
            setWorkspaceAgents((prev) => ({
              ...prev,
              [workspaceUuid]: (prev[workspaceUuid] ?? []).filter(
                (agent) => agent.agent_uuid !== normalized.agent_uuid,
              ),
            }));
            setSelectedAgent((current) =>
              current?.agent_uuid === normalized.agent_uuid ? null : current,
            );
            if (selectedAgentUuidRef.current === normalized.agent_uuid) {
              closeAgentStream();
              setChat(initialChatState());
              setConnectionStatus("idle");
            }
            return;
          }
          if (normalized.type === "error") {
            setError(normalized.message);
          }
          // TODO: handle context_created / workflow_created / workflow_started / workflow_cancel_requested
          // when frontend has UI for resources and workflows
        };

        for (const eventName of WORKSPACE_EVENT_NAMES) {
          stream.addEventListener(eventName, consume(eventName));
        }
        stream.onerror = () => {
          if (!connected) {
            stream.close();
            delete workspaceStreamsRef.current[workspaceUuid];
            reject(new Error("Workspace is already in use or unavailable"));
            return;
          }
          setWorkspaceSessions((prev) => {
            const next = { ...prev };
            delete next[workspaceUuid];
            return next;
          });
          setError("Workspace event stream disconnected");
        };
      });

      return access;
    },
    [clientId, closeAgentStream],
  );

  const openAgentStream = useCallback(
    async (agent: AgentSummary, access: WorkspaceAccess) => {
      closeAgentStream();
      setSelectedAgent(agent);
      setConnectionStatus("connecting");
      setError(null);
      ignoreStreamUntilNextStatusRef.current = false;
      const agentAccess = { ...access, agent_uuid: agent.agent_uuid };
      const snapshot = await agentSnapshot(agentAccess);
      setChat({
        ...initialChatState(),
        units: snapshot.units,
        status: snapshot.status,
        pendingApproval: snapshot.pending_approval,
      });

      const stream = new EventSource(agentEventStreamUrl(agentAccess));
      agentStreamRef.current = stream;
      const consume = (eventName: string) => (event: MessageEvent) => {
        const normalized = normalizeAgentEvent({ eventName, data: event.data });
        if (normalized.type === "connected") {
          setConnectionStatus("connected");
          return;
        }
        if (normalized.type === "stream_delta" && ignoreStreamUntilNextStatusRef.current) {
          return;
        }
        if (normalized.type === "status_changed") {
          ignoreStreamUntilNextStatusRef.current = false;
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
    [closeAgentStream],
  );

  const selectAgent = useCallback(
    async (agent: AgentSummary) => {
      const wsUuid = Object.entries(workspaceAgents).find(([, agents]) =>
        agents.some((candidate) => candidate.agent_uuid === agent.agent_uuid),
      )?.[0];
      if (!wsUuid) {
        return;
      }
      const access = await openWorkspaceStream(wsUuid);
      await openAgentStream(agent, access);
    },
    [openAgentStream, openWorkspaceStream, workspaceAgents],
  );

  const expandWorkspace = useCallback(
    async (workspace: WorkspaceSummary) => {
      if (expandedWorkspaceUuids.includes(workspace.workspace_uuid)) {
        setExpandedWorkspaceUuids((prev) =>
          prev.filter((id) => id !== workspace.workspace_uuid),
        );
        setWorkspaceAgents((prev) => {
          const next = { ...prev };
          delete next[workspace.workspace_uuid];
          return next;
        });
        if (
          selectedAgent &&
          workspaceAgents[workspace.workspace_uuid]?.some(
            (agent) => agent.agent_uuid === selectedAgent.agent_uuid,
          )
        ) {
          closeAgentStream();
          setSelectedAgent(null);
          setChat(initialChatState());
        }
        closeWorkspaceStream(workspace.workspace_uuid);
        return;
      }

      const access = await openWorkspaceStream(workspace.workspace_uuid);
      setExpandedWorkspaceUuids((prev) => [...prev, workspace.workspace_uuid]);
      setError(null);

      const agents = await listAgents(access);
      setWorkspaceAgents((prev) => ({ ...prev, [workspace.workspace_uuid]: agents }));
    },
    [
      closeAgentStream,
      closeWorkspaceStream,
      expandedWorkspaceUuids,
      openWorkspaceStream,
      selectedAgent,
      workspaceAgents,
    ],
  );

  const loadInitialData = useCallback(async () => {
    setError(null);
    const [workspaceList, profileList] = await Promise.all([listWorkspaces(), listProfiles()]);
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
      const access = await openWorkspaceStream(wsUuid);
      await createAgentApi(access, input);
    },
    [openWorkspaceStream],
  );

  const deleteAgent = useCallback(
    async (agent: AgentSummary) => {
      const wsUuid = Object.entries(workspaceAgents).find(([, agents]) =>
        agents.some((candidate) => candidate.agent_uuid === agent.agent_uuid),
      )?.[0];
      if (!wsUuid) {
        return;
      }
      const access = await openWorkspaceStream(wsUuid);
      await deleteAgentApi({ ...access, agent_uuid: agent.agent_uuid });
    },
    [openWorkspaceStream, workspaceAgents],
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
        await sendMessage(
          {
            workspace_uuid: activeSession.workspace_uuid,
            client_id: clientId,
            agent_uuid: selectedAgent.agent_uuid,
          },
          text,
        );
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
    [activeSession, clientId, selectedAgent],
  );

  const approve = useCallback(
    async (approvalMask: number) => {
      if (!selectedAgent || !chat.pendingApproval || !activeSession) {
        return;
      }
      const access: AgentAccess = {
        workspace_uuid: activeSession.workspace_uuid,
        client_id: clientId,
        agent_uuid: selectedAgent.agent_uuid,
      };
      await approveRequest(access, chat.pendingApproval.request_uuid, approvalMask);
      setChat((current) => ({ ...current, pendingApproval: null, streamingText: "" }));
    },
    [activeSession, chat.pendingApproval, clientId, selectedAgent],
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
    const access: AgentAccess = {
      workspace_uuid: activeSession.workspace_uuid,
      client_id: clientId,
      agent_uuid: selectedAgent.agent_uuid,
    };

    if (status === "running_llm") {
      ignoreStreamUntilNextStatusRef.current = true;
      setChat((current) => ({
        ...current,
        units: current.pendingUserUuid
          ? current.units.filter((unit) => unit.uuid !== current.pendingUserUuid)
          : current.units,
        pendingUserUuid: null,
        streamingText: "",
      }));
      try {
        await cancelAgent(access);
      } catch (err) {
        ignoreStreamUntilNextStatusRef.current = false;
        throw err;
      }
      return;
    }

    setChat((current) => ({ ...current, streamingText: "" }));
    await cancelAgent(access);
  }, [activeSession, approve, chat.status, clientId, selectedAgent]);

  useEffect(
    () => () => {
      closeAgentStream();
      for (const stream of Object.values(workspaceStreamsRef.current)) {
        stream.close();
      }
      workspaceStreamsRef.current = {};
    },
    [closeAgentStream],
  );

  return {
    clientId,
    workspaces,
    profiles,
    expandedWorkspaceUuids,
    workspaceSessions,
    workspaceAgents,
    selectedAgent,
    session: activeSession,
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
    deleteAgent,
    send,
    cancel,
    approve,
  };
}
