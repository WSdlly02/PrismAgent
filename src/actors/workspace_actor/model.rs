use crate::error::SubsystemResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

pub const WORKSPACE_ACTOR: &str = "workspace";

#[derive(Clone)]
pub struct WorkspaceHandle {
    pub tx: mpsc::Sender<WorkspaceMsg>,
}

pub struct WorkspaceActor {
    pub(super) rx: mpsc::Receiver<WorkspaceMsg>,
    pub(super) root: PathBuf,
    pub(super) workspaces: HashMap<String, Workspace>,
}

pub enum WorkspaceMsg {
    List {
        reply: oneshot::Sender<SubsystemResult<Vec<WorkspaceSummary>>>,
    },
    Create {
        request: WorkspaceCreateRequest,
        reply: oneshot::Sender<SubsystemResult<WorkspaceSummary>>,
    },
    Contains {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<bool>>,
    },
    Get {
        workspace_uuid: String,
        reply: oneshot::Sender<SubsystemResult<Workspace>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub uuid: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCreateRequest {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_uuid: String,
    pub workspace_path: PathBuf,
    pub locked_by: Option<String>,
}
