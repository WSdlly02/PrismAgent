use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::tools_actor::builtin_provider::BuiltinToolProvider;
use crate::actors::tools_actor::model::{
    TOOLS_ACTOR, ToolApproval, ToolBatchRequest, ToolBatchResponse, ToolExecutionContext,
    ToolStreamEvent, ToolsActor, ToolsHandle, ToolsMsg,
};
use crate::actors::tools_actor::provider::{ToolProvider, ToolResult, ToolRouter, ToolSpec};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::impl_handle_methods;
use genai::chat::{ChatMessage, Tool, ToolCall, ToolResponse};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

impl ToolsActor {
    pub fn load(rx: mpsc::Receiver<ToolsMsg>, handles: AppHandles) -> SubsystemResult<Self> {
        let providers: Vec<Arc<dyn ToolProvider>> = vec![Arc::new(BuiltinToolProvider::load())];
        Ok(Self {
            rx,
            handles,
            router: Arc::new(ToolRouter::new(providers)?),
            inflight: HashMap::new(),
        })
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
                    let router = self.router.clone();
                    let task = tokio::spawn(async move {
                        let result = dispatch_batch(handles, router, request).await;
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
        self.router
            .resolve(names.as_deref())
            .into_iter()
            .map(to_genai_tool)
            .collect()
    }

    fn prune_finished(&mut self) {
        self.inflight.retain(|_, task| !task.is_finished());
    }
}

fn to_genai_tool(spec: &ToolSpec) -> Tool {
    Tool {
        name: spec.name.clone().into(),
        description: spec.description.clone(),
        schema: Some(spec.input_schema.clone()),
        strict: spec.strict,
        config: None,
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
    router: Arc<ToolRouter>,
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
                &router,
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
    router: &ToolRouter,
    ctx: ToolExecutionContext,
    index: usize,
    tool_call: &ToolCall,
    approval: &ToolApproval,
    stream_tx: &mpsc::Sender<ToolStreamEvent>,
) -> ToolResponse {
    if !approval.approved {
        let content = ToolResult::error(json!({
            "status": "denied",
            "reason": approval
                .reason
                .clone()
                .unwrap_or_else(|| "tool execution was not approved".to_string()),
        }))
        .into_response_content();
        return ToolResponse::from_tool_call(tool_call, content);
    }

    let _ = stream_tx
        .send(ToolStreamEvent::ToolStarted {
            index,
            name: tool_call.fn_name.clone(),
        })
        .await;
    let output = match router
        .call(
            tool_call.fn_name.clone(),
            ctx,
            tool_call.fn_arguments.clone(),
        )
        .await
    {
        Ok(output) => output,
        Err(error) => ToolResult::error(json!({
            "error": error.to_string(),
        })),
    }
    .into_response_content();
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
