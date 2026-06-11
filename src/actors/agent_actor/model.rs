use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest};
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use genai::chat::ToolCall;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

pub const AGENT_ACTOR: &str = "agent";

#[derive(Clone)]
pub struct AgentHandle {
    pub tx: mpsc::Sender<AgentMsg>,
}

pub struct AgentActor {
    pub(super) rx: mpsc::Receiver<AgentMsg>,
    pub(super) agents: HashMap<String, Agent>, // agent_uuid -> Agent
    pub(super) agent_workspace: HashMap<String, String>, // agent_uuid -> workspace_uuid
    pub(super) runtimes: HashMap<String, AgentRuntime>, // agent_uuid -> AgentRuntime
    pub(super) handles: AppHandles,
}

pub enum AgentMsg {
    List {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<Vec<AgentSummary>>>,
    },
    Create {
        request: AgentCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    Delete {
        workspace_uuid: String,
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    Contains {
        workspace_uuid: String,
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<bool>>,
    },
    Snapshot {
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<AgentSnapshot>>,
    },
    SendMessage {
        request: SendMessageRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    SelfUpdate {
        request: SelfUpdateRequest,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    ApproveRequest {
        request: ApproveRequest,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    Cancel {
        agent_uuid: String,
        reply: oneshot::Sender<SubsystemResult<()>>,
    },
    SetAutoLoop {
        agent_uuid: String,
        enabled: bool,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    InferenceFinished {
        agent_uuid: String,
        inference_uuid: String,
        result: SubsystemResult<AgentInferenceOutput>,
    },
    ToolBatchFinished {
        agent_uuid: String,
        job_uuid: String,
        result: SubsystemResult<ToolBatchOutput>,
    },
}

pub struct AgentRuntime {
    pub status: AgentStatus,
    pub inference_uuid: Option<String>,
    pub pending_tool_batch: Option<PendingToolBatch>,
    pub active_tool_batch: Option<PendingToolBatch>,
    pub malformed_tool_call_retries: u8,
}

impl AgentRuntime {
    pub fn idle() -> Self {
        Self {
            status: AgentStatus::Idle,
            inference_uuid: None,
            pending_tool_batch: None,
            active_tool_batch: None,
            malformed_tool_call_retries: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub units: Vec<Unit>,
    pub status: AgentStatus,
    pub pending_approval: Option<PendingApproval>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub agent_uuid: String,
    pub agent_name: String,
    pub profile: String,
    pub auto_loop: bool,
    pub context_refs: Vec<String>,
    pub context_out: Vec<String>,
    pub status: AgentStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    UnitAppend { unit: Unit },
    StreamDelta { text: String },
    ApproveRequest { request: PendingApproval },
    StatusChanged { status: AgentStatus },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    RunningLlm,
    RunningTool,
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
pub struct SelfUpdateRequest {
    pub agent_uuid: String,
    pub context_refs: Option<Vec<String>>,
    pub context_out: Option<Vec<String>>,
    pub auto_loop: Option<bool>,
    pub auto_loop_message: Option<String>,
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
    pub approval_mask: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub request_uuid: String,
    pub description: String,
    pub tool_count: usize,
    pub auto_approved_mask: u64,
    pub manual_approval_mask: u64,
}

pub struct AgentInferenceOutput {
    pub units: Vec<Unit>,
    pub is_tool_calls: bool,
}

pub struct PendingToolBatch {
    pub request_uuid: String,
    pub tool_calls: Vec<ToolCall>,
    pub auto_approved_mask: u64,
    pub manual_approval_mask: u64,
}

pub struct ToolBatchOutput {
    pub units: Vec<Unit>,
    pub continue_loop: bool,
}
