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
    pub(super) workspaces: HashMap<String, WorkspaceState>,
}

pub enum WorkspaceMsg {
    List {
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
}

pub struct WorkspaceState {
    pub uuid: String,
    pub path: PathBuf,
    pub agents: Vec<AgentSummary>,
    pub lease: Option<Lease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_uuid: String,
    pub workspace_path: PathBuf,
    pub locked_by: Option<String>,
    pub agents: Vec<AgentSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSummary {
    pub agent_uuid: String,
    pub agent_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquireLeaseRequest {
    pub workspace_uuid: String,
    pub client_id: String,
    pub lease_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseLeaseRequest {
    pub workspace_uuid: String,
    pub lease_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lease {
    pub lease_token: String,
    pub workspace_uuid: String,
    pub client_id: String,
    pub expires_at: i64,
}
