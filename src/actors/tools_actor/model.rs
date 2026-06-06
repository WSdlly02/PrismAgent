use crate::actors::storage_actor::model::unit::Unit;
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use genai::chat::{Tool, ToolCall};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};

pub const TOOLS_ACTOR: &str = "tools";

#[derive(Clone)]
pub struct ToolsHandle {
    pub tx: mpsc::Sender<ToolsMsg>,
}

pub struct ToolsActor {
    pub(super) rx: mpsc::Receiver<ToolsMsg>,
    pub(super) handles: AppHandles,
    pub(super) tools: Vec<Tool>,
    pub(super) tools_map: HashMap<String, ToolExecutor>,
    pub(super) inflight: HashMap<String, tokio::task::JoinHandle<()>>,
}

pub enum ToolsMsg {
    List {
        names: Option<Vec<String>>,
        reply: oneshot::Sender<SubsystemResult<Vec<Tool>>>,
    },
    DispatchBatch {
        request: ToolBatchRequest,
        reply: oneshot::Sender<SubsystemResult<ToolBatchResponse>>,
    },
    Cancel {
        job_uuid: String,
        reply: oneshot::Sender<SubsystemResult<bool>>,
    },
}

pub type ToolFuture = Pin<Box<dyn Future<Output = String> + Send>>; // type alias for boxed future of string result, since async fn pointers are not directly supported in traits or structs
pub type ToolExecutor = fn(ToolExecutionContext, Value) -> ToolFuture; // fn pointer to async function that takes context and args, returns future of string result

#[derive(Clone)]
pub struct ToolExecutionContext {
    pub handles: AppHandles,
    pub workspace_uuid: String,
    pub caller_agent_uuid: String,
    pub workspace_path: PathBuf,
}

pub struct ToolBatchRequest {
    pub job_uuid: String,
    pub workspace_uuid: String,
    pub caller_agent_uuid: String,
    pub workspace_path: PathBuf,
    pub tool_calls: Vec<ToolCall>,
    pub approvals: Vec<ToolApproval>,
    pub stream_tx: mpsc::Sender<ToolStreamEvent>,
}

pub struct ToolBatchResponse {
    pub output_units: Vec<Unit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolApproval {
    pub approved: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolStreamEvent {
    Started { tool_count: usize },
    ToolStarted { index: usize, name: String },
    ToolFinished { index: usize, name: String },
    Finished,
}
