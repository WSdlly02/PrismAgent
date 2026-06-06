use crate::actors::context_actor::model::{
    CONTEXT_ACTOR, ContextActor, ContextHandle, ContextMsg, ReadSkillRequest,
    RenderCapabilitiesRequest, RenderInitialPromptsRequest, ResolveContextRefsRequest,
    SkillDescriptor, SkillScope,
};
use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use genai::chat::ChatMessage;
use std::path::PathBuf;
use tokio::sync::mpsc;

impl ContextActor {
    pub fn load(rx: mpsc::Receiver<ContextMsg>, handles: AppHandles) -> Self {
        Self { rx, handles }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ContextMsg::ReadSkill { request, reply } => {
                    let _ = reply.send(read_skill(request));
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
        request: RenderInitialPromptsRequest,
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
    pub async fn read_skill(
        &self,
        request_body: ReadSkillRequest,
    ) -> SubsystemResult<SkillDescriptor> {
        request(&self.tx, |reply| ContextMsg::ReadSkill {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn render_initial_prompts(
        &self,
        request_body: RenderInitialPromptsRequest,
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
            format!(
                "- {} ({:?}):\n{}",
                skill.name,
                skill.scope,
                skill.frontmatter.trim()
            )
        })
        .collect::<Vec<_>>();
    if !skills.is_empty() {
        sections.push(format!("Available skills:\n{}", skills.join("\n\n")));
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

fn read_skill(request: ReadSkillRequest) -> SubsystemResult<SkillDescriptor> {
    let workspace_path = workspace_skill_path(&request.workspace_uuid, &request.name)?;
    if workspace_path.is_file() {
        return read_skill_file(request.name, SkillScope::Workspace, workspace_path);
    }
    let global_path = global_skill_path(&request.name)?;
    if global_path.is_file() {
        return read_skill_file(request.name, SkillScope::Global, global_path);
    }
    Err(SubsystemError::not_found("skill", request.name))
}

fn list_skills(workspace_uuid: &str) -> SubsystemResult<Vec<SkillDescriptor>> {
    let mut skills = Vec::new();
    collect_skills(
        SkillScope::Global,
        prismagent_root()?.join("skills"),
        &mut skills,
    )?;
    collect_skills(
        SkillScope::Workspace,
        workspaces_root()?.join(workspace_uuid).join("skills"),
        &mut skills,
    )?;
    skills.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(skills)
}

fn collect_skills(
    scope: SkillScope,
    root: PathBuf,
    skills: &mut Vec<SkillDescriptor>,
) -> SubsystemResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        let skill_path = path.join("SKILL.md");
        if !skill_path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        skills.push(read_skill_file(
            name.to_string(),
            scope.clone(),
            skill_path,
        )?);
    }
    Ok(())
}

fn read_skill_file(
    name: String,
    scope: SkillScope,
    path: PathBuf,
) -> SubsystemResult<SkillDescriptor> {
    let data = std::fs::read_to_string(&path)
        .map_err(|error| SubsystemError::io(format!("{}: {error}", path.display())))?;
    let frontmatter = extract_frontmatter(&data).ok_or_else(|| {
        SubsystemError::invalid_input(format!("{}: missing frontmatter", path.display()))
    })?;
    Ok(SkillDescriptor {
        name,
        scope,
        path,
        frontmatter,
        content: strip_frontmatter(&data).trim().to_string(),
    })
}

fn extract_frontmatter(data: &str) -> Option<String> {
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

fn strip_frontmatter(data: &str) -> &str {
    let Some(rest) = data.strip_prefix("---") else {
        return data;
    };
    let Some((_, content)) = rest.split_once("\n---") else {
        return data;
    };
    content
}

fn workspace_skill_path(workspace_uuid: &str, name: &str) -> SubsystemResult<PathBuf> {
    Ok(workspaces_root()?
        .join(workspace_uuid)
        .join("skills")
        .join(name)
        .join("SKILL.md"))
}

fn global_skill_path(name: &str) -> SubsystemResult<PathBuf> {
    Ok(prismagent_root()?
        .join("skills")
        .join(name)
        .join("SKILL.md"))
}

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
        let data = "---\nname: demo\ndescription: test\n---\nbody";
        assert_eq!(
            extract_frontmatter(data).unwrap(),
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
}
