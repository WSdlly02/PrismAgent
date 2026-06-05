use crate::actors::storage_actor::model::context::{Context, ContextCreateRequest};
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowCreateRequest};
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    pub(super) triggers: HashMap<String, WorkflowTrigger>, // trigger_uuid -> WorkflowTrigger
}

pub enum WorkflowMsg {
    UuidNew {
        count: usize,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    WorkflowNew {
        request: WorkflowCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Workflow>>,
    },
    WorkflowRun {
        request: WorkflowRunRequest,
        reply: oneshot::Sender<SubsystemResult<WorkflowRuntime>>,
    },
    ContextNew {
        request: ContextCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Context>>,
    },
    TriggerNew {
        request: WorkflowTriggerCreateRequest,
        reply: oneshot::Sender<SubsystemResult<WorkflowTrigger>>,
    },
    TaskFinished {
        request: TaskFinishedRequest,
        reply: oneshot::Sender<SubsystemResult<TaskFinishedResponse>>,
    },
    ListContextOut {
        request: ListContextOutRequest,
        reply: oneshot::Sender<SubsystemResult<ListContextOutResponse>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRuntime {
    pub workspace_uuid: String,
    pub workflow_uuid: String,
    pub coordinator_agent_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunRequest {
    pub workspace_uuid: String,
    pub workflow_uuid: String,
    pub coordinator_uuid: String,
    pub coordinator_name: String,
    #[serde(default = "default_coordinator_profile")]
    pub coordinator_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    pub uuid: String,
    pub workspace_uuid: String,
    pub workflow_uuid: String,
    pub coordinator_agent_uuid: String,
    pub context_uuids: Vec<String>, // 触发Workflow所需的Context UUID列表
    pub fired_context_uuids: HashSet<String>, // 已经触发过Workflow的Context UUID列表
    pub message: String,            // 触发Workflow的消息描述
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTriggerCreateRequest {
    pub workspace_uuid: String,
    pub uuid: String,
    pub workflow_uuid: String,
    pub coordinator_agent_uuid: String,
    pub context_uuids: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFinishedRequest {
    pub workspace_uuid: String,
    pub agent_uuid: String,
    pub summary: String,
    #[serde(default)]
    pub context_outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFinishedResponse {
    pub agent_uuid: String,
    pub auto_loop: bool,
    pub summary: String,
    pub context_outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListContextOutRequest {
    pub workspace_uuid: String,
    pub agent_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextOutStatus {
    pub context_uuid: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListContextOutResponse {
    pub agent_uuid: String,
    pub context_out: Vec<ContextOutStatus>,
}

fn default_coordinator_profile() -> String {
    "coordinator".to_string()
}
