mod fs;
mod prismagent;
mod shell;
mod web;

use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::actors::tools_actor::provider::{ToolExecutor, ToolFuture, ToolProvider, ToolSpec};
use crate::error::SubsystemError;
use serde_json::Value;
use std::collections::HashMap;

macro_rules! tool_entry {
    ($module:ident::$def_fn:ident / $exec_fn:ident) => {{
        fn execute(ctx: ToolExecutionContext, args: Value) -> ToolFuture {
            Box::pin(async move { Ok($module::$exec_fn(ctx, args).await) })
        }
        ($module::$def_fn(), execute as ToolExecutor)
    }};
}

macro_rules! register_tools {
    ($($module:ident::$def_fn:ident / $exec_fn:ident),* $(,)?) => {
        fn registered_tool_entries() -> Vec<(ToolSpec, ToolExecutor)> {
            vec![
                $(tool_entry!($module::$def_fn / $exec_fn),)*
            ]
        }
    };
}

register_tools! {
    fs::dir_list / execute_dir_list,
    fs::tree_list / execute_tree_list,
    fs::path_stat / execute_path_stat,
    fs::file_read / execute_file_read,
    fs::file_write / execute_file_write,
    fs::file_replace / execute_file_replace,
    fs::dir_create / execute_dir_create,
    fs::path_remove / execute_path_remove,
    fs::path_rename / execute_path_rename,
    fs::file_copy / execute_file_copy,
    shell::exec / execute,
    web::search / execute_search,
    web::fetch / execute_fetch,
    prismagent::uuid_generate / execute_uuid_generate,
    prismagent::agent_list / execute_agent_list,

    prismagent::context_create / execute_context_create,
    // Context content is injected while an agent is created.
    prismagent::workflow_create / execute_workflow_create,
    // Workflows are executed directly by WorkflowActor.
    prismagent::workflow_start / execute_workflow_start,

    prismagent::skill_dir_get / execute_skill_dir_get,
    prismagent::profile_list / execute_profile_list,

    // Equivalent to agent_list for the calling agent.
    prismagent::self_show / execute_self_show,
    prismagent::self_update / execute_self_update,
    prismagent::task_finish / execute_task_finish,
}

pub struct BuiltinToolProvider {
    tools: Vec<ToolSpec>,
    executors: HashMap<String, ToolExecutor>,
}

impl BuiltinToolProvider {
    pub fn load() -> Self {
        let entries = registered_tool_entries();
        let mut tools = Vec::with_capacity(entries.len());
        let mut executors = HashMap::with_capacity(entries.len());
        for (tool, executor) in entries {
            executors.insert(tool.name.clone(), executor);
            tools.push(tool);
        }
        Self { tools, executors }
    }
}

impl ToolProvider for BuiltinToolProvider {
    fn id(&self) -> &str {
        "builtin"
    }

    fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    fn call(&self, tool_name: String, ctx: ToolExecutionContext, arguments: Value) -> ToolFuture {
        let Some(executor) = self.executors.get(&tool_name).copied() else {
            return Box::pin(async move {
                Err(SubsystemError::validation(format!(
                    "builtin provider does not own tool: {tool_name}"
                )))
            });
        };
        Box::pin(async move { executor(ctx, arguments).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_provider_has_one_executor_for_each_tool() {
        let provider = BuiltinToolProvider::load();

        assert!(!provider.tools.is_empty());
        assert_eq!(provider.tools.len(), provider.executors.len());
        assert!(
            provider
                .tools
                .iter()
                .all(|tool| provider.executors.contains_key(&tool.name))
        );
    }
}
