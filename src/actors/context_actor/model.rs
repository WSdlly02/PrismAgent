use crate::actors::profile_actor::model::Profile;
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
    ReadSkill {
        request: ReadSkillRequest,
        reply: oneshot::Sender<SubsystemResult<SkillDescriptor>>,
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
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Global,
    Workspace,
}
