use crate::actors::agent_actor::model::AgentHandle;
use crate::actors::context_actor::model::ContextHandle;
use crate::actors::llm_actor::model::LlmHandle;
use crate::actors::profile_actor::model::{ProfileHandle, ProfileMsg};
use crate::actors::shell_actor::model::ShellHandle;
use crate::actors::storage_actor::model::StorageHandle;
use crate::actors::tools_actor::model::ToolsHandle;
use crate::actors::workspace_actor::model::WorkspaceHandle;

#[derive(Clone)]
pub struct AppHandles {
    pub profile: ProfileHandle,
    pub context: ContextHandle,
    pub storage: StorageHandle,
    pub workspace: WorkspaceHandle,
    pub agent: AgentHandle,
    pub shell: ShellHandle,
    pub llm: LlmHandle,
    pub tools: ToolsHandle,
}

impl AppHandles {
    /// Temporary bootstrap helper until ProfileActor is part of daemon startup.
    pub fn without_profile_actor(
        context: ContextHandle,
        storage: StorageHandle,
        workspace: WorkspaceHandle,
        agent: AgentHandle,
        shell: ShellHandle,
        llm: LlmHandle,
        tools: ToolsHandle,
    ) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<ProfileMsg>(1);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        Self {
            profile: ProfileHandle { tx },
            context,
            storage,
            workspace,
            agent,
            shell,
            llm,
            tools,
        }
    }
}

#[cfg(test)]
pub fn test_handles() -> AppHandles {
    use crate::actors::agent_actor::model::AgentMsg;
    use crate::actors::llm_actor::model::LlmMsg;
    use crate::actors::shell_actor::model::ShellMsg;
    use crate::actors::tools_actor::model::ToolsMsg;
    use crate::actors::workspace_actor::model::WorkspaceMsg;

    let (context, _) = tokio::sync::mpsc::channel(1);
    let (storage, _) = tokio::sync::mpsc::channel(1);
    let (workspace, _) = tokio::sync::mpsc::channel::<WorkspaceMsg>(1);
    let (agent, _) = tokio::sync::mpsc::channel::<AgentMsg>(1);
    let (shell, _) = tokio::sync::mpsc::channel::<ShellMsg>(1);
    let (llm, _) = tokio::sync::mpsc::channel::<LlmMsg>(1);
    let (tools, _) = tokio::sync::mpsc::channel::<ToolsMsg>(1);
    AppHandles::without_profile_actor(
        ContextHandle { tx: context },
        StorageHandle { tx: storage },
        WorkspaceHandle { tx: workspace },
        AgentHandle { tx: agent },
        ShellHandle { tx: shell },
        LlmHandle { tx: llm },
        ToolsHandle { tx: tools },
    )
}
