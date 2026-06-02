use crate::actors::agent_actor::model::{
    AgentEvent, AgentSnapshot, ApproveRequest, SendMessageRequest,
};
use crate::actors::workspace_actor::model::{
    AcquireLeaseRequest, Lease, ReleaseLeaseRequest, WorkspaceSummary,
};
use crate::error::SubsystemResult;
use tokio::sync::{broadcast, mpsc, oneshot};

pub const SHELL_ACTOR: &str = "shell";

#[derive(Clone)]
pub struct ShellHandle {
    pub tx: mpsc::Sender<ShellMsg>,
}

pub struct ShellActor {
    pub(super) rx: mpsc::Receiver<ShellMsg>,
    pub(super) workspace: crate::actors::workspace_actor::model::WorkspaceHandle,
    pub(super) agent: crate::actors::agent_actor::model::AgentHandle,
}

pub enum ShellMsg {
    ListWorkspaces {
        reply: oneshot::Sender<SubsystemResult<Vec<WorkspaceSummary>>>,
    },
    AcquireLease {
        request: AcquireLeaseRequest,
        reply: oneshot::Sender<SubsystemResult<Lease>>,
    },
    ReleaseLease {
        request: ReleaseLeaseRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    AgentSnapshot {
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<AgentSnapshot>>,
    },
    SubscribeAgent {
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<broadcast::Receiver<AgentEvent>>>,
    },
    SendMessage {
        request: SendMessageRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    ApproveRequest {
        request: ApproveRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    Cancel {
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
}
