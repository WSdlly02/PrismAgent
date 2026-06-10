use crate::actors::agent_actor::model::{AgentEvent, AgentSnapshot, AgentSummary, MessageBody};
use crate::actors::storage_actor::model::agent::Agent;
use crate::actors::workflow_actor::model::WorkflowCancelResponse;
use crate::actors::workspace_actor::model::{WorkspaceCreateRequest, WorkspaceSummary};
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

pub const SHELL_ACTOR: &str = "shell";

#[derive(Clone)]
pub struct ShellHandle {
    pub tx: mpsc::Sender<ShellMsg>,
}

pub struct ShellActor {
    pub(super) rx: mpsc::Receiver<ShellMsg>,
    pub(super) handles: AppHandles,
    pub(super) workspace_subscribers: HashMap<String, WorkspaceSubscription>, // workspace_uuid -> subscription
    pub(super) subscribers: HashMap<String, mpsc::Sender<AgentEvent>>, // subscriber_id -> Sender<AgentEvent>
}

pub struct WorkspaceSubscription {
    pub client_id: String,
    pub tx: mpsc::Sender<WorkspaceEvent>,
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
    SubscribeWorkspace {
        request: WorkspaceSubscribeRequest,
        reply: oneshot::Sender<SubsystemResult<mpsc::Receiver<WorkspaceEvent>>>,
    },
    UnsubscribeWorkspace {
        request: WorkspaceSubscribeRequest,
    },
    ListAgents {
        request: WorkspaceAccessRequest,
        reply: oneshot::Sender<SubsystemResult<Vec<AgentSummary>>>,
    },
    CreateAgent {
        request: AuthorizedAgentCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    DeleteAgent {
        request: AgentAccessRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    AgentSnapshot {
        request: AgentAccessRequest,
        reply: oneshot::Sender<SubsystemResult<AgentSnapshot>>,
    },
    SubscribeAgent {
        request: AgentAccessRequest,
        reply: oneshot::Sender<SubsystemResult<mpsc::Receiver<AgentEvent>>>,
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
        request: AgentAccessRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    WorkflowCancel {
        request: AuthorizedWorkflowCancelRequest,
        reply: oneshot::Sender<SubsystemResult<WorkflowCancelResponse>>,
    },
    EmitAgentEvent {
        agent_uuid: String,
        event: AgentEvent,
    },
    EmitWorkspaceEvent {
        workspace_uuid: String,
        event: WorkspaceEvent,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSubscribeRequest {
    pub workspace_uuid: String,
    pub client_id: String,
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
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedSendMessageRequest {
    #[serde(flatten)]
    pub access: AgentAccessRequest,
    pub message_body: MessageBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedApproveRequest {
    #[serde(flatten)]
    pub access: AgentAccessRequest,
    pub request_uuid: String,
    pub approval_mask: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizedWorkflowCancelRequest {
    #[serde(flatten)]
    pub workspace: WorkspaceAccessRequest,
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
    pub workspace: WorkspaceAccessRequest,
    #[serde(flatten)]
    pub agent: AgentCreateBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkspaceEvent {
    AgentCreated {
        agent: AgentSummary,
    },
    AgentUpdated {
        agent: AgentSummary,
    },
    AgentStatusChanged {
        agent_uuid: String,
        status: crate::actors::agent_actor::model::AgentStatus,
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
        coordinator_agent_uuid: String,
    },
    WorkflowCancelRequested {
        workflow_uuid: String,
        coordinator_agent_uuid: String,
    },
}
