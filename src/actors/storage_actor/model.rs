use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

use crate::actors::storage_actor::model::agent::{Agent, AgentReplaceEntry};
use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::misc::{MiscReadEntry, MiscReplaceEntry, MiscWriteEntry};
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowReplaceEntry};
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
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadAgents {
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Agent>>>,
    },
    WriteAgents {
        agents: Vec<Agent>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReplaceAgents {
        entries: Vec<AgentReplaceEntry>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ListUnits {
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadUnits {
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Unit>>>,
    },
    WriteUnits {
        units: Vec<Unit>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ListContexts {
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadContexts {
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Context>>>,
    },
    WriteContexts {
        contexts: Vec<Context>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ListWorkflows {
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadWorkflows {
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Workflow>>>,
    },
    WriteWorkflows {
        workflows: Vec<Workflow>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReplaceWorkflows {
        entries: Vec<WorkflowReplaceEntry>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ListMisc {
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadMisc {
        names: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<MiscReadEntry>>>,
    },
    WriteMisc {
        entries: Vec<MiscWriteEntry>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReplaceMisc {
        entries: Vec<MiscReplaceEntry>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
}
