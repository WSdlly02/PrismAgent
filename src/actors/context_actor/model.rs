use crate::actors::profile_actor::model::Profile;
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use serde::{Deserialize, Serialize};
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
    GetSkillDir {
        request: GetSkillDirRequest,
        reply: oneshot::Sender<SubsystemResult<String>>,
    },
    RenderInitialPrompts {
        request: Box<RenderInitialPromptsRequest>,
        reply: oneshot::Sender<SubsystemResult<Vec<Unit>>>,
    },
}

#[derive(Debug, Clone)]
pub struct RenderInitialPromptsRequest {
    pub workspace_uuid: String,
    pub agent_uuid: String,
    pub context_refs: Vec<String>,
    pub profile: Profile,
}

#[derive(Debug, Clone)]
pub struct RenderCapabilitiesRequest {
    pub workspace_uuid: String,
    pub profile: Profile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveContextRefsRequest {
    pub workspace_uuid: String,
    pub context_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSkillDirRequest {
    pub workspace_uuid: String,
    pub name: String,
}

/// 描述一个已发现的 skill，用于在 agent 的 prompt 中列出可用技能。
/// `content`（SKILL.md 正文）不再由系统读取，agent 可通过 get_skill_dir 获取路径后用 FS 工具自行阅读。
#[derive(Debug, Clone)]
pub struct SkillDescriptor {
    pub name: String,
    pub scope: SkillScope,
    /// SKILL.md 的 YAML frontmatter，简洁描述该技能的用途
    pub frontmatter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Global,
    Workspace,
}
