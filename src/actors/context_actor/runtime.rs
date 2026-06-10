use crate::actors::context_actor::model::{
    CONTEXT_ACTOR, ContextActor, ContextHandle, ContextMsg, GetSkillDirRequest,
    RenderCapabilitiesRequest, RenderInitialPromptsRequest, ResolveContextRefsRequest,
    SkillDescriptor, SkillScope,
};
use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::stdlib_assets;
use genai::chat::ChatMessage;
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::sync::mpsc;

impl ContextActor {
    pub fn load(rx: mpsc::Receiver<ContextMsg>, handles: AppHandles) -> SubsystemResult<Self> {
        // 启动时一次 bootstrap，之后 list_skills / get_skill_dir 都是纯文件系统操作
        let root = prismagent_root()?;
        stdlib_assets::bootstrap_skills(&root.join("skills"))?;
        Ok(Self { rx, handles })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ContextMsg::GetSkillDir { request, reply } => {
                    let _ = reply.send(get_skill_dir(request));
                }
                ContextMsg::RenderInitialPrompts { request, reply } => {
                    let _ = reply.send(self.render_initial_prompts(request).await);
                }
            }
        }
    }

    async fn resolve_context_refs(
        &self,
        request: ResolveContextRefsRequest,
    ) -> SubsystemResult<Vec<Context>> {
        if request.context_refs.is_empty() {
            return Ok(Vec::new());
        }
        self.handles
            .storage
            .read_contexts(request.workspace_uuid, request.context_refs)
            .await
    }

    async fn render_initial_prompts(
        &self,
        request: Box<RenderInitialPromptsRequest>,
    ) -> SubsystemResult<Vec<Unit>> {
        let capabilities = render_capabilities(RenderCapabilitiesRequest {
            workspace_uuid: request.workspace_uuid.clone(),
            profile: request.profile.clone(),
        })?;
        let system = render_system_prompt(&request.profile, &request.agent_uuid, &capabilities);
        let mut system_unit = Unit::from_chat_message(ChatMessage::system(system));
        system_unit.visibility =
            crate::actors::storage_actor::model::unit::UnitVisibility::Internal;
        let mut units = vec![system_unit];

        if !request.context_refs.is_empty() {
            let contexts = self
                .resolve_context_refs(ResolveContextRefsRequest {
                    workspace_uuid: request.workspace_uuid,
                    context_refs: request.context_refs,
                })
                .await?;
            units.push(Unit::from_chat_message(ChatMessage::user(
                render_task_context(&contexts),
            )));
        }

        Ok(units)
    }
}

impl ContextHandle {
    pub async fn get_skill_dir(&self, request_body: GetSkillDirRequest) -> SubsystemResult<String> {
        request(&self.tx, |reply| ContextMsg::GetSkillDir {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn render_initial_prompts(
        &self,
        request_body: Box<RenderInitialPromptsRequest>,
    ) -> SubsystemResult<Vec<Unit>> {
        request(&self.tx, |reply| ContextMsg::RenderInitialPrompts {
            request: request_body,
            reply,
        })
        .await
    }
}

async fn request<T>(
    tx: &mpsc::Sender<ContextMsg>,
    message: impl FnOnce(tokio::sync::oneshot::Sender<SubsystemResult<T>>) -> ContextMsg,
) -> SubsystemResult<T> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(message(reply_tx))
        .await
        .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?;
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(CONTEXT_ACTOR))?
}

// ---------------------------------------------------------------------------
// get_skill_dir：校验 skill 目录存在且无路径逃逸，返回路径字符串
// ---------------------------------------------------------------------------

fn get_skill_dir(request: GetSkillDirRequest) -> SubsystemResult<String> {
    let name = request.name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(SubsystemError::invalid_input(format!(
            "invalid skill name: {name}"
        )));
    }

    // 先查 workspace 级
    let ws_root = workspaces_root()?
        .join(&request.workspace_uuid)
        .join("skills");
    let ws_path = ws_root.join(name);
    if ws_path.is_dir() && is_subdir(&ws_path, &ws_root)? {
        return Ok(ws_path.to_string_lossy().into_owned());
    }

    // 再查全局级
    let global_root = prismagent_root()?.join("skills");
    let global_path = global_root.join(name);
    if global_path.is_dir() && is_subdir(&global_path, &global_root)? {
        return Ok(global_path.to_string_lossy().into_owned());
    }

    Err(SubsystemError::not_found("skill", name))
}

/// 校验 resolved 路径确实在 base 目录之下，防止路径逃逸。
fn is_subdir(resolved: &PathBuf, base: &PathBuf) -> SubsystemResult<bool> {
    let resolved_canonical = std::fs::canonicalize(resolved)
        .map_err(|error| SubsystemError::io(format!("failed to resolve path: {error}")))?;
    let base_canonical = std::fs::canonicalize(base)
        .map_err(|error| SubsystemError::io(format!("failed to resolve base path: {error}")))?;
    Ok(resolved_canonical.starts_with(&base_canonical))
}

// workspace 级 skill 可覆盖同名的 global skill
// ---------------------------------------------------------------------------

fn list_skills(workspace_uuid: &str) -> SubsystemResult<Vec<SkillDescriptor>> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut skills = Vec::new();

    // workspace 级优先
    collect_skills(
        SkillScope::Workspace,
        workspaces_root()?.join(workspace_uuid).join("skills"),
        &mut skills,
        &mut seen,
    )?;
    // global 级补充，同名则跳过（workspace 覆盖）
    collect_skills(
        SkillScope::Global,
        prismagent_root()?.join("skills"),
        &mut skills,
        &mut seen,
    )?;

    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

fn collect_skills(
    scope: SkillScope,
    root: PathBuf,
    skills: &mut Vec<SkillDescriptor>,
    seen: &mut HashSet<String>,
) -> SubsystemResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !seen.insert(name.to_string()) {
            continue; // 已存在同名 skill，跳过
        }
        let frontmatter = extract_frontmatter(&skill_md).unwrap_or_default();
        skills.push(SkillDescriptor {
            name: name.to_string(),
            scope: scope.clone(),
            frontmatter,
        });
    }
    Ok(())
}

fn extract_frontmatter(path: &PathBuf) -> Option<String> {
    let data = std::fs::read_to_string(path).ok()?;
    let mut lines = data.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut frontmatter = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Some(frontmatter.join("\n"));
        }
        frontmatter.push(line.to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// Prompt 渲染
// ---------------------------------------------------------------------------

fn render_system_prompt(
    profile: &crate::actors::profile_actor::model::Profile,
    agent_uuid: &str,
    capabilities: &str,
) -> String {
    let system = &profile.prompts.system;
    [
        &format!("# Runtime Identity\n\nagent_uuid: {agent_uuid}"),
        system.identity.trim(),
        system.behavior.trim(),
        system.response_style.trim(),
        &system
            .capabilities
            .replace("{skills} {tools}", capabilities),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn render_task_context(contexts: &[Context]) -> String {
    let rendered = contexts
        .iter()
        .map(|context| format!("## {}\n\n{}", context.title.trim(), context.content.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("# Task Context\n\n{rendered}")
}

fn render_capabilities(request: RenderCapabilitiesRequest) -> SubsystemResult<String> {
    let mut sections = Vec::new();

    let skills = list_skills(&request.workspace_uuid)?
        .into_iter()
        .map(|skill| {
            let scope_tag = match skill.scope {
                SkillScope::Global => "[global]",
                SkillScope::Workspace => "[workspace]",
            };
            if skill.frontmatter.is_empty() {
                format!("- {scope_tag} {}", skill.name)
            } else {
                format!(
                    "- {scope_tag} {}:\n{}",
                    skill.name,
                    skill.frontmatter.trim()
                )
            }
        })
        .collect::<Vec<_>>();
    if !skills.is_empty() {
        sections.push(format!("Available skills:\n{}", skills.join("\n\n")));
        sections.push(
            "Use prismagent_skill_dir_get to get the path of a skill directory, \
             then read its files with fs_file_read/fs_tree_list."
                .to_string(),
        );
    }

    if !request.profile.tools.available_tools.is_empty() {
        sections.push(format!(
            "Available tools: {}.",
            request.profile.tools.available_tools.join(", ")
        ));
    }
    if request.profile.tools.yolo {
        sections.push("Tool approval mode: yolo.".to_string());
    } else if !request.profile.tools.auto_approve.is_empty() {
        sections.push(format!(
            "Auto-approved tools: {}.",
            request.profile.tools.auto_approve.join(", ")
        ));
    } else {
        sections.push("Tool approval mode: ask before tool execution.".to_string());
    }
    Ok(sections.join("\n\n"))
}

// ---------------------------------------------------------------------------
// 路径工具
// ---------------------------------------------------------------------------

fn workspaces_root() -> SubsystemResult<PathBuf> {
    Ok(prismagent_root()?.join("workspaces"))
}

fn prismagent_root() -> SubsystemResult<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| SubsystemError::internal("failed to determine home directory"))?
        .join(".prismagent"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_frontmatter() {
        let dir = std::env::temp_dir().join(format!("skill-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SKILL.md");
        std::fs::write(&path, "---\nname: demo\ndescription: test\n---\nbody").unwrap();

        assert_eq!(
            extract_frontmatter(&path).unwrap(),
            "name: demo\ndescription: test"
        );
    }

    #[test]
    fn renders_task_context_documents() {
        let rendered = render_task_context(&[Context {
            uuid: "ctx".to_string(),
            title: "Task".to_string(),
            content: "Do the thing.".to_string(),
            created_at: 0,
        }]);
        assert!(rendered.contains("# Task Context"));
        assert!(rendered.contains("## Task"));
        assert!(rendered.contains("Do the thing."));
    }

    #[test]
    fn rejects_path_traversal_in_skill_name() {
        let result = get_skill_dir(GetSkillDirRequest {
            workspace_uuid: "ws".to_string(),
            name: "../foo".to_string(),
        });
        assert!(result.is_err());
        match result {
            Err(crate::error::SubsystemError::InvalidInput { message }) => {
                assert!(message.contains("../foo"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn not_found_for_chinese_skill_name() {
        let result = get_skill_dir(GetSkillDirRequest {
            workspace_uuid: "ws".to_string(),
            name: "知识".to_string(),
        });
        match result {
            Err(crate::error::SubsystemError::NotFound { resource, .. }) => {
                assert_eq!(resource, "skill");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
