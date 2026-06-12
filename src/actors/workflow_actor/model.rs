use crate::actors::agent_actor::model::AgentStatus;
use crate::actors::storage_actor::model::context::{Context, ContextCreateRequest};
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowCreateRequest};
use crate::actors::workflow_actor::dag::WorkflowRuntime;
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

pub const WORKFLOW_ACTOR: &str = "workflow";

#[derive(Clone)]
pub struct WorkflowHandle {
    pub tx: mpsc::Sender<WorkflowMsg>,
}

pub struct WorkflowActor {
    pub(super) rx: mpsc::Receiver<WorkflowMsg>,
    pub(super) handles: AppHandles,
    pub(super) runtimes: HashMap<(String, String), WorkflowRuntime>, // (workspace_uuid, workflow_uuid) -> WorkflowRuntime
}

pub enum WorkflowMsg {
    UuidGenerate {
        count: usize,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    WorkflowCreate {
        request: WorkflowCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Workflow>>,
    },
    WorkflowStart {
        request: WorkflowStartRequest,
        reply: oneshot::Sender<SubsystemResult<WorkflowRuntime>>,
    },
    WorkflowCancel {
        request: WorkflowCancelRequest,
        reply: oneshot::Sender<SubsystemResult<WorkflowCancelResponse>>,
    },
    ContextCreate {
        request: ContextCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Context>>,
    },
    TaskFinish {
        request: TaskFinishRequest,
        reply: oneshot::Sender<SubsystemResult<TaskFinishResponse>>,
    },
    SelfShow {
        request: SelfShowRequest,
        reply: oneshot::Sender<SubsystemResult<SelfShowResponse>>,
    },
    ListAgents {
        request: ListAgentsRequest,
        reply: oneshot::Sender<SubsystemResult<ListAgentsResponse>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStartRequest {
    pub workspace_uuid: String,
    pub workflow_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCancelRequest {
    pub workspace_uuid: String,
    pub workflow_uuid: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCancelResponse {
    pub workspace_uuid: String,
    pub workflow_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFinishRequest {
    pub workspace_uuid: String,
    pub agent_uuid: String,
    pub summary: String,
    #[serde(default)]
    pub context_outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFinishResponse {
    pub agent_uuid: String,
    pub auto_loop: bool,
    pub summary: String,
    pub context_outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfShowRequest {
    pub workspace_uuid: String,
    pub agent_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAgentsRequest {
    pub workspace_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextStatus {
    pub context_uuid: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentView {
    pub agent_uuid: String,
    pub name: String,
    pub profile: String,
    pub auto_loop: bool,
    pub status: AgentStatus,
    pub context_refs: Vec<ContextStatus>,
    pub context_out: Vec<ContextStatus>,
}

pub type SelfShowResponse = AgentView;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAgentsResponse {
    pub agents: Vec<AgentView>,
}
