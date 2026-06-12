use crate::actors::agent_actor::model::{AgentSnapshot, AgentSummary, PendingApproval};
use crate::actors::storage_actor::model::agent::Agent;
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::workflow_actor::model::WorkflowCancelResponse;
use crate::actors::workspace_actor::model::{
    AcquireLeaseRequest, Lease, ReleaseLeaseRequest, WorkspaceCreateRequest, WorkspaceSummary,
};
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

use crate::actors::agent_actor::model::AgentStatus;

pub const SHELL_ACTOR: &str = "shell";

/// Opaque connection identifier assigned by the WS handler.
pub type ConnectionId = u64;

#[derive(Clone)]
pub struct ShellHandle {
    pub tx: mpsc::Sender<ShellMsg>,
}

pub struct ShellActor {
    pub(super) rx: mpsc::Receiver<ShellMsg>,
    pub(super) handles: AppHandles,
    pub(super) connections: HashMap<ConnectionId, ConnectionSession>,
    pub(super) connection_channels: HashMap<ConnectionId, mpsc::Sender<WsEvent>>,
    pub(super) leases: HashMap<String, Lease>,
    pub(super) workspace_subscribers: HashMap<String, Vec<ConnectionId>>, // multi-reader
    pub(super) agent_subscribers: HashMap<String, Vec<ConnectionId>>,     // multi-reader
}

pub struct ConnectionSession {
    pub connection_id: ConnectionId,
    pub subscribed_workspace: Option<String>,
    pub subscribed_agent: Option<String>,
}

/// Routing target for a WS event.
pub enum EventTarget {
    Workspace(String),
    Agent(String),
}

/// Unified WS event (merged WorkspaceEvent + AgentEvent).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    // ---- Workspace events ----
    AgentCreated {
        agent: AgentSummary,
    },
    AgentUpdated {
        agent: AgentSummary,
    },
    AgentStatusChanged {
        agent_uuid: String,
        status: AgentStatus,
    },
    AgentDeleted {
        agent_uuid: String,
    },
    ContextCreated {
        context_uuid: String,
        title: String,
    },
    WorkflowCreated {
        workflow_uuid: String,
        title: String,
    },
    WorkflowStarted {
        workflow_uuid: String,
        planner_agent_uuid: String,
    },
    WorkflowCancelRequested {
        workflow_uuid: String,
        planner_agent_uuid: String,
    },
    WorkspaceDeleted {
        workspace_uuid: String,
    },

    // ---- Agent events ----
    UnitAppend {
        unit: Unit,
    },
    StreamDelta {
        text: String,
    },
    ApproveRequest {
        request: PendingApproval,
    },
    StatusChanged {
        status: AgentStatus,
    },

    // ---- Common ----
    Error {
        message: String,
    },
}

pub enum ShellMsg {
    ListWorkspaces {
        reply: oneshot::Sender<SubsystemResult<Vec<WorkspaceSummary>>>,
    },
    ListProfiles {
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    CreateWorkspace {
        request: WorkspaceCreateRequest,
        reply: oneshot::Sender<SubsystemResult<WorkspaceSummary>>,
    },
    AcquireLease {
        request: AcquireLeaseRequest,
        reply: oneshot::Sender<SubsystemResult<Lease>>,
    },
    ReleaseLease {
        request: ReleaseLeaseRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },

    // ---- Connection lifecycle ----
    RegisterConnection {
        connection_id: ConnectionId,
        reply: oneshot::Sender<mpsc::Receiver<WsEvent>>,
    },
    UnregisterConnection {
        connection_id: ConnectionId,
    },

    // ---- Workspace subscription (multi-reader) ----
    SubscribeWorkspace {
        connection_id: ConnectionId,
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    UnsubscribeWorkspace {
        connection_id: ConnectionId,
    },

    // ---- Agent subscription (single-reader per agent) ----
    SubscribeAgent {
        connection_id: ConnectionId,
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    UnsubscribeAgent {
        connection_id: ConnectionId,
    },

    // ---- REST operations ----
    ListAgents {
        request: WorkspaceAccessRequest,
        reply: oneshot::Sender<SubsystemResult<Vec<AgentSummary>>>,
    },
    CreateAgent {
        request: AuthorizedAgentCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    DeleteAgent {
        request: AgentWriteAccessRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    AgentSnapshot {
        request: AgentAccessRequest,
        reply: oneshot::Sender<SubsystemResult<AgentSnapshot>>,
    },
    SendMessage {
        request: AuthorizedSendMessageRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    ApproveRequest {
        request: AuthorizedApproveRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    Cancel {
        request: AgentWriteAccessRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    CancelWorkflow {
        request: AuthorizedCancelWorkflowRequest,
        reply: oneshot::Sender<SubsystemResult<WorkflowCancelResponse>>,
    },
    DeleteWorkspace {
        request: AuthorizedDeleteWorkspaceRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },

    // ---- Event emission ----
    EmitEvent {
        target: EventTarget,
        event: WsEvent,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAccessRequest {
    #[serde(flatten)]
    pub workspace: WorkspaceAccessRequest,
    pub agent_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceAccessRequest {
    pub workspace_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceWriteAccessRequest {
    pub workspace_uuid: String,
    pub lease_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedDeleteWorkspaceRequest {
    pub workspace_uuid: String,
    pub lease_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWriteAccessRequest {
    #[serde(flatten)]
    pub workspace: WorkspaceWriteAccessRequest,
    pub agent_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedSendMessageRequest {
    #[serde(flatten)]
    pub access: AgentWriteAccessRequest,
    pub message_body: crate::actors::agent_actor::model::MessageBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedApproveRequest {
    #[serde(flatten)]
    pub access: AgentWriteAccessRequest,
    pub request_uuid: String,
    pub approval_mask: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedCancelWorkflowRequest {
    #[serde(flatten)]
    pub workspace: WorkspaceWriteAccessRequest,
    pub workflow_uuid: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCreateBody {
    pub name: String,
    pub profile: String,
    #[serde(default)]
    pub context_refs: Vec<String>,
    #[serde(default)]
    pub context_out: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedAgentCreateRequest {
    #[serde(flatten)]
    pub workspace: WorkspaceWriteAccessRequest,
    #[serde(flatten)]
    pub agent: AgentCreateBody,
}
