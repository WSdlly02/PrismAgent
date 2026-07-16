use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::tools_actor::model::{
    TOOLS_ACTOR, ToolApproval, ToolBatchRequest, ToolBatchResponse, ToolExecutionContext,
    ToolExecutor, ToolFuture, ToolStreamEvent, ToolsActor, ToolsHandle, ToolsMsg,
};
use crate::actors::tools_actor::{fs, prismagent, shell, web};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::impl_handle_methods;
use genai::chat::{ChatMessage, Tool, ToolCall, ToolResponse};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;

pub fn tool_template(name: &str, description: &str, schema: Value) -> Tool {
    Tool {
        name: name.into(),
        description: Some(description.into()),
        schema: Some(schema),
        strict: Some(true),
        config: None,
    }
}

macro_rules! tool_entry {
    ($module:ident::$def_fn:ident / $exec_fn:ident) => {{
        fn execute(ctx: ToolExecutionContext, args: Value) -> ToolFuture {
            Box::pin(async move { $module::$exec_fn(ctx, args).await })
        }
        ($module::$def_fn(), execute as ToolExecutor)
    }};
}

macro_rules! register_tools {
    ($($module:ident::$def_fn:ident / $exec_fn:ident),* $(,)?) => {
        fn registered_tool_entries() -> Vec<(Tool, ToolExecutor)> {
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
    // context_read is not needed
    // the content of context file will be injected when creating the agent, using render_initial_prompts to render the context units
    prismagent::workflow_create / execute_workflow_create,
    // workflow_read is not needed
    // the content of workflow file will be executed directly by the workflow actor, without being parsed by the agent, so no need to read the workflow content in the agent

    prismagent::workflow_start / execute_workflow_start,

    prismagent::skill_dir_get / execute_skill_dir_get,
    prismagent::profile_list / execute_profile_list,

    // equivalent to agent_list
    prismagent::self_show / execute_self_show,
    prismagent::self_update / execute_self_update,
    prismagent::task_finish / execute_task_finish,
}

impl ToolsActor {
    pub fn load(rx: mpsc::Receiver<ToolsMsg>, handles: AppHandles) -> Self {
        let entries = registered_tool_entries();
        let mut tools = Vec::with_capacity(entries.len());
        let mut tools_map = HashMap::with_capacity(entries.len());
        for (tool, executor) in entries {
            tools_map.insert(tool.name.to_string(), executor);
            tools.push(tool);
        }
        Self {
            rx,
            handles,
            tools,
            tools_map,
            inflight: HashMap::new(),
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            self.prune_finished();
            match msg {
                ToolsMsg::List { names, reply } => {
                    let _ = reply.send(Ok(self.list_tools(names)));
                }
                ToolsMsg::DispatchBatch { request, reply } => {
                    let Some(job_uuid) = non_empty(request.job_uuid.clone()) else {
                        let _ = reply.send(Err(SubsystemError::validation(
                            "tool job_uuid must not be empty",
                        )));
                        continue;
                    };
                    let handles = self.handles.clone();
                    let tools_map = self.tools_map.clone();
                    let task = tokio::spawn(async move {
                        let result = dispatch_batch(handles, tools_map, request).await;
                        let _ = reply.send(result);
                    });
                    self.inflight.insert(job_uuid, task);
                }
                ToolsMsg::Cancel { job_uuid, reply } => {
                    let cancelled = self
                        .inflight
                        .remove(&job_uuid)
                        .map(|task| {
                            task.abort();
                            true
                        })
                        .unwrap_or(false);
                    let _ = reply.send(Ok(cancelled));
                }
            }
        }
    }

    fn list_tools(&self, names: Option<Vec<String>>) -> Vec<Tool> {
        let Some(names) = names else {
            return self.tools.clone();
        };
        if names.iter().any(|name| name == "*") {
            return self.tools.clone();
        }
        let allowed = names.into_iter().collect::<HashSet<_>>();
        self.tools
            .iter()
            .filter(|tool| allowed.contains(&tool.name.to_string()))
            .cloned()
            .collect()
    }

    fn prune_finished(&mut self) {
        self.inflight.retain(|_, task| !task.is_finished());
    }
}

// ---- Declarative macro: handle methods with concrete types ----

impl_handle_methods! {
    ToolsHandle for ToolsMsg, TOOLS_ACTOR;

    fn list(&self, names: Option<Vec<String>>) -> Vec<Tool>
        => List { names: names };

    fn dispatch_batch(&self, request: ToolBatchRequest) -> ToolBatchResponse
        => DispatchBatch { request: request };

    fn cancel(&self, job_uuid: impl Into<String>) -> bool
        => Cancel { job_uuid: job_uuid.into() };
}

async fn dispatch_batch(
    handles: AppHandles,
    tools_map: HashMap<String, ToolExecutor>,
    request: ToolBatchRequest,
) -> SubsystemResult<ToolBatchResponse> {
    if request.approvals.len() != request.tool_calls.len() {
        return Err(SubsystemError::validation(format!(
            "approval count {} does not match tool call count {}",
            request.approvals.len(),
            request.tool_calls.len()
        )));
    }
    let _ = request
        .stream_tx
        .send(ToolStreamEvent::Started {
            tool_count: request.tool_calls.len(),
        })
        .await;
    let ctx = ToolExecutionContext {
        handles,
        workspace_uuid: request.workspace_uuid,
        caller_agent_uuid: request.caller_agent_uuid,
        workspace_path: request.workspace_path,
    };
    let mut responses = Vec::with_capacity(request.tool_calls.len());
    for (index, (tool_call, approval)) in request
        .tool_calls
        .iter()
        .zip(request.approvals.iter())
        .enumerate()
    {
        responses.push(
            execute_one(
                &tools_map,
                ctx.clone(),
                index,
                tool_call,
                approval,
                &request.stream_tx,
            )
            .await,
        );
    }
    let _ = request.stream_tx.send(ToolStreamEvent::Finished).await;
    let output_units = responses
        .into_iter()
        .map(|response| Unit::from_chat_message(ChatMessage::from(response)))
        .collect();
    Ok(ToolBatchResponse { output_units })
}

async fn execute_one(
    tools_map: &HashMap<String, ToolExecutor>,
    ctx: ToolExecutionContext,
    index: usize,
    tool_call: &ToolCall,
    approval: &ToolApproval,
    stream_tx: &mpsc::Sender<ToolStreamEvent>,
) -> ToolResponse {
    if !approval.approved {
        let content = json!({
            "status": "denied",
            "reason": approval
                .reason
                .clone()
                .unwrap_or_else(|| "tool execution was not approved".to_string()),
        })
        .to_string();
        return ToolResponse::from_tool_call(tool_call, content);
    }

    let _ = stream_tx
        .send(ToolStreamEvent::ToolStarted {
            index,
            name: tool_call.fn_name.clone(),
        })
        .await;
    let output = match tools_map.get(&tool_call.fn_name).copied() {
        Some(executor) => executor(ctx, tool_call.fn_arguments.clone()).await,
        None => json!({
            "status": "error",
            "error": format!("unknown tool: {}", tool_call.fn_name),
        })
        .to_string(),
    };
    let _ = stream_tx
        .send(ToolStreamEvent::ToolFinished {
            index,
            name: tool_call.fn_name.clone(),
        })
        .await;
    ToolResponse::from_tool_call(tool_call, output)
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
