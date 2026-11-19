use crate::actors::agent_actor::model::AgentHandle;
use crate::actors::context_actor::model::ContextHandle;
use crate::actors::llm_actor::model::LlmHandle;
use crate::actors::profile_actor::model::ProfileHandle;
use crate::actors::shell_actor::model::ShellHandle;
use crate::actors::storage_actor::model::StorageHandle;
use crate::actors::tools_actor::model::ToolsHandle;
use crate::actors::workflow_actor::model::WorkflowHandle;
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
    pub workflow: WorkflowHandle,
}

#[cfg(test)]
pub fn test_handles() -> AppHandles {
    use crate::actors::agent_actor::model::AgentMsg;
    use crate::actors::llm_actor::model::LlmMsg;
    use crate::actors::shell_actor::model::ShellMsg;
    use crate::actors::tools_actor::model::ToolsMsg;
    use crate::actors::workflow_actor::model::WorkflowMsg;
    use crate::actors::workspace_actor::model::WorkspaceMsg;

    let (profile, _) = tokio::sync::mpsc::channel(1);
    let (context, _) = tokio::sync::mpsc::channel(1);
    let (storage, _) = tokio::sync::mpsc::channel(1);
    let (workspace, _) = tokio::sync::mpsc::channel::<WorkspaceMsg>(1);
    let (agent, _) = tokio::sync::mpsc::channel::<AgentMsg>(1);
    let (shell, _) = tokio::sync::mpsc::channel::<ShellMsg>(1);
    let (llm, _) = tokio::sync::mpsc::channel::<LlmMsg>(1);
    let (tools, _) = tokio::sync::mpsc::channel::<ToolsMsg>(1);
    let (workflow, _) = tokio::sync::mpsc::channel::<WorkflowMsg>(1);
    AppHandles {
        profile: ProfileHandle { tx: profile },
        context: ContextHandle { tx: context },
        storage: StorageHandle { tx: storage },
        workspace: WorkspaceHandle { tx: workspace },
        agent: AgentHandle { tx: agent },
        shell: ShellHandle { tx: shell },
        llm: LlmHandle { tx: llm },
        tools: ToolsHandle { tx: tools },
        workflow: WorkflowHandle { tx: workflow },
    }
}
