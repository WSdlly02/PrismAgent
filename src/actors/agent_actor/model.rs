use crate::actors::agent_actor::state::AgentEntry;
use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest};
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
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
    pub(super) entries: HashMap<String, AgentEntry>, // agent_uuid -> AgentEntry
    pub(super) handles: AppHandles,
}

pub enum AgentMsg {
    TryShutdown {
        reply: oneshot::Sender<SubsystemResult<bool>>,
    },
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
    ForgetWorkspace {
        workspace_uuid: String,
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
        result: AgentTaskResult<AgentInferenceOutput>,
    },
    ToolBatchFinished {
        agent_uuid: String,
        job_uuid: String,
        result: AgentTaskResult<ToolBatchOutput>,
    },
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

/// Status of an Agent for representation in the UI.
///
/// It is not a complete representation of the Agent's internal state, but rather a simplified view for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    RunningLlm,
    RunningTool,
    WaitingApproval,
}

/// Exact point in the asynchronous Agent flow where a failure occurred.
///
/// A single stage avoids the invalid combinations that separate operation and
/// phase enums allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFailureStage {
    ReadHistory,
    BuildInput,
    LoadToolWorkspace,
    LoadModelConfig,
    LoadToolsConfig,
    ResolveTools,
    ProviderInference,
    PrepareToolBatch,
    DispatchTools,
    RepairToolCalls,
    CommitLlmOutput,
    CommitToolOutput,
    PrepareAutoLoop,
    ApplyNextAction,
}

/// Adds flow context to an internal error from a background Agent task. It is
/// converted to a public WS event only at the Agent boundary.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("agent flow failed during {stage:?}: {source}")]
pub struct AgentTaskError {
    pub stage: AgentFailureStage,
    #[source]
    pub source: SubsystemError,
}

impl AgentTaskError {
    pub fn new(stage: AgentFailureStage, source: SubsystemError) -> Self {
        Self { stage, source }
    }
}

pub type AgentTaskResult<T> = Result<T, AgentTaskError>;

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

pub struct ToolBatchOutput {
    pub units: Vec<Unit>,
    pub continue_loop: bool,
}
