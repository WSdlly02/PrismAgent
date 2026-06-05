use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest};
use crate::actors::storage_actor::model::context::{Context, ContextCreateRequest};
use crate::actors::storage_actor::model::misc::{MiscReadEntry, MiscReplaceEntry, MiscWriteEntry};
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::storage_actor::model::workflow::{
    Workflow, WorkflowCreateRequest, WorkflowReplaceEntry,
};
use crate::error::SubsystemResult;

pub mod agent;
pub mod context;
pub mod misc;
pub mod unit;
pub mod workflow;

pub const STORAGE_ACTOR: &str = "storage";

#[derive(Clone)]
pub struct StorageHandle {
    pub tx: mpsc::Sender<StorageMsg>,
}

pub struct StorageActor {
    pub(super) rx: mpsc::Receiver<StorageMsg>,
    pub(super) root: PathBuf,
}

pub enum StorageMsg {
    Root {
        reply: oneshot::Sender<SubsystemResult<PathBuf>>,
    },
    ListAgents {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadAgents {
        workspace_uuid: String,
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Agent>>>,
    },
    WriteAgents {
        workspace_uuid: String,
        agents: Vec<Agent>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    CreateAgent {
        request: AgentCreateRequest,
        auto_loop: bool,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    SetAgentAutoLoop {
        workspace_uuid: String,
        agent_uuid: String,
        enabled: bool,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    AppendAgentUnits {
        workspace_uuid: String,
        agent_uuid: String,
        units: Vec<Unit>,
        reply: oneshot::Sender<SubsystemResult<Agent>>,
    },
    ListUnits {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadUnits {
        workspace_uuid: String,
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Unit>>>,
    },
    WriteUnits {
        workspace_uuid: String,
        units: Vec<Unit>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ListContexts {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadContexts {
        workspace_uuid: String,
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Context>>>,
    },
    WriteContexts {
        workspace_uuid: String,
        contexts: Vec<Context>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    CreateContext {
        request: ContextCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Context>>,
    },
    ListWorkflows {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadWorkflows {
        workspace_uuid: String,
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Workflow>>>,
    },
    WriteWorkflows {
        workspace_uuid: String,
        workflows: Vec<Workflow>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    CreateWorkflow {
        request: WorkflowCreateRequest,
        reply: oneshot::Sender<SubsystemResult<Workflow>>,
    },
    ReplaceWorkflows {
        workspace_uuid: String,
        entries: Vec<WorkflowReplaceEntry>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ListMisc {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadMisc {
        workspace_uuid: String,
        names: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<MiscReadEntry>>>,
    },
    WriteMisc {
        workspace_uuid: String,
        entries: Vec<MiscWriteEntry>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReplaceMisc {
        workspace_uuid: String,
        entries: Vec<MiscReplaceEntry>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
}
