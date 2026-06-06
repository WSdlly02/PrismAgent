use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::tools_actor::model::{
    TOOLS_ACTOR, ToolApproval, ToolBatchRequest, ToolBatchResponse, ToolExecutionContext,
    ToolExecutor, ToolFuture, ToolStreamEvent, ToolsActor, ToolsHandle, ToolsMsg,
};
use crate::actors::tools_actor::{fs, prismagent, shell, web};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
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
    fs::ls_tree / execute_ls_tree,
    fs::read / execute_read,
    fs::list / execute_list,
    fs::stat / execute_stat,
    fs::write / execute_write,
    fs::replace / execute_replace,
    fs::mkdir / execute_mkdir,
    fs::remove / execute_remove,
    fs::rename / execute_rename,
    fs::copy / execute_copy,
    shell::exec / execute,
    web::search / execute_search,
    web::fetch / execute_fetch,
    prismagent::uuid_new / execute_uuid_new,
    prismagent::read_skill / execute_read_skill,
    prismagent::agent_new / execute_agent_new,
    prismagent::context_new / execute_context_new,
    prismagent::workflow_new / execute_workflow_new,
    prismagent::workflow_run / execute_workflow_run,
    prismagent::trigger_new / execute_trigger_new,
    prismagent::list_profiles / execute_list_profiles,
    prismagent::show_myself / execute_show_myself,
    prismagent::task_finished / execute_task_finished,
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
                        let _ = reply.send(Err(SubsystemError::invalid_input(
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

impl ToolsHandle {
    pub async fn list(&self, names: Option<Vec<String>>) -> SubsystemResult<Vec<Tool>> {
        request_response(&self.tx, |reply| ToolsMsg::List { names, reply }).await
    }

    pub async fn dispatch_batch(
        &self,
        request: ToolBatchRequest,
    ) -> SubsystemResult<ToolBatchResponse> {
        request_response(&self.tx, |reply| ToolsMsg::DispatchBatch { request, reply }).await
    }

    pub async fn cancel(&self, job_uuid: impl Into<String>) -> SubsystemResult<bool> {
        request_response(&self.tx, |reply| ToolsMsg::Cancel {
            job_uuid: job_uuid.into(),
            reply,
        })
        .await
    }
}

async fn request_response<T>(
    tx: &mpsc::Sender<ToolsMsg>,
    message: impl FnOnce(tokio::sync::oneshot::Sender<SubsystemResult<T>>) -> ToolsMsg,
) -> SubsystemResult<T> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(message(reply_tx))
        .await
        .map_err(|_| SubsystemError::actor_dead(TOOLS_ACTOR))?;
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(TOOLS_ACTOR))?
}

async fn dispatch_batch(
    handles: AppHandles,
    tools_map: HashMap<String, ToolExecutor>,
    request: ToolBatchRequest,
) -> SubsystemResult<ToolBatchResponse> {
    if request.approvals.len() != request.tool_calls.len() {
        return Err(SubsystemError::invalid_input(format!(
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
