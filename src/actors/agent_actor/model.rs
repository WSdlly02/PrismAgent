use crate::error::SubsystemResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::{broadcast, mpsc, oneshot};

pub const AGENT_ACTOR: &str = "agent";

#[derive(Clone)]
pub struct AgentHandle {
    pub tx: mpsc::Sender<AgentMsg>,
}

pub struct AgentActor {
    pub(super) rx: mpsc::Receiver<AgentMsg>,
    pub(super) agents: HashMap<String, AgentState>,
}

pub enum AgentMsg {
    Snapshot {
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<AgentSnapshot>>,
    },
    Subscribe {
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

pub struct AgentState {
    pub uuid: String,
    pub name: String,
    pub units: Vec<AgentUnit>,
    pub status: AgentStatus,
    pub events: broadcast::Sender<AgentEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub agent_uuid: String,
    pub agent_name: String,
    pub units: Vec<AgentUnit>,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentUnit {
    pub unit_uuid: String,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    UnitAppend { unit: AgentUnit },
    ApproveRequest { request: PendingApproval },
    StatusChanged { status: AgentStatus },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Running,
    WaitingApproval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub agent_uuid: String,
    pub message_body: MessageBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBody {
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub data: String,
    pub filename: String,
    pub mimetype: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproveRequest {
    pub agent_uuid: String,
    pub request_uuid: String,
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub request_uuid: String,
    pub description: String,
}
