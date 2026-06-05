use crate::actors::profile_actor::model::Profile;
use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

pub const CONTEXT_ACTOR: &str = "context";

#[derive(Clone)]
pub struct ContextHandle {
    pub tx: mpsc::Sender<ContextMsg>,
}

pub struct ContextActor {
    pub(super) rx: mpsc::Receiver<ContextMsg>,
    pub(super) handles: AppHandles,
}

pub enum ContextMsg {
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
    ResolveContextRefs {
        request: ResolveContextRefsRequest,
        reply: oneshot::Sender<SubsystemResult<Vec<Context>>>,
    },
    RenderTaskContext {
        contexts: Vec<Context>,
        reply: oneshot::Sender<SubsystemResult<String>>,
    },
    ReadSkill {
        request: ReadSkillRequest,
        reply: oneshot::Sender<SubsystemResult<SkillDescriptor>>,
    },
    RenderCapabilities {
        request: RenderCapabilitiesRequest,
        reply: oneshot::Sender<SubsystemResult<String>>,
    },
    RenderInitialPrompts {
        request: RenderInitialPromptsRequest,
        reply: oneshot::Sender<SubsystemResult<Vec<Unit>>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveContextRefsRequest {
    pub workspace_uuid: String,
    pub context_refs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RenderInitialPromptsRequest {
    pub workspace_uuid: String,
    pub context_refs: Vec<String>,
    pub profile: Profile,
}

#[derive(Debug, Clone)]
pub struct RenderCapabilitiesRequest {
    pub workspace_uuid: String,
    pub profile: Profile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadSkillRequest {
    pub workspace_uuid: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDescriptor {
    pub name: String,
    pub scope: SkillScope,
    pub path: PathBuf,
    pub frontmatter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Global,
    Workspace,
}
