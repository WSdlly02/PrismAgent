use crate::actors::profile_actor::model::DEFAULT_PROFILE_NAME;
use crate::actors::storage_actor::model::agent::AgentCreateRequest;
use crate::actors::storage_actor::model::context::ContextCreateRequest;
use crate::actors::storage_actor::model::workflow::WorkflowCreateRequest;
use crate::actors::tools_actor::model::ToolExecutionContext;
use crate::actors::tools_actor::runtime::tool_template;
use genai::chat::Tool;
use serde_json::{Value, json};
use std::collections::HashMap;

pub fn agent_new() -> Tool {
    tool_template(
        "prismagent_agent_new",
        "Create a new PrismAgent agent in the current workspace. The returned uuid is a petname id for later references.",
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Human-readable agent name describing its responsibility"},
                "profile": {"type": "string", "description": "Profile name, e.g. default, planner, executor"},
                "context_refs": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Context uuids this agent should use as task input"
                },
                "context_out": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Context uuids this agent is expected to produce"
                }
            },
            "required": ["name"]
        }),
    )
}

pub fn context_new() -> Tool {
    tool_template(
        "prismagent_context_new",
        "Create a new context document in the current workspace. The returned uuid is a petname id for later references.",
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "Human-readable context title"},
                "content": {"type": "string", "description": "Markdown context content"}
            },
            "required": ["title", "content"]
        }),
    )
}

pub fn workflow_new() -> Tool {
    tool_template(
        "prismagent_workflow_new",
        "Create a new workflow document in the current workspace. The returned uuid is a petname id for later references.",
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "Human-readable workflow title"},
                "content": {"type": "string", "description": "Workflow description, preferably markdown with Mermaid when useful"},
                "metadata": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Optional workflow metadata"
                }
            },
            "required": ["title", "content"]
        }),
    )
}

pub async fn execute_agent_new(ctx: ToolExecutionContext, args: Value) -> String {
    let request = AgentCreateRequest {
        workspace_uuid: ctx.workspace_uuid.clone(),
        name: string_arg(&args, "name").unwrap_or_else(|| "Untitled agent".to_string()),
        profile: string_arg(&args, "profile").unwrap_or_else(|| DEFAULT_PROFILE_NAME.to_string()),
        context_refs: string_array_arg(&args, "context_refs"),
        context_out: string_array_arg(&args, "context_out"),
    };
    match ctx.handles.agent.create(request).await {
        Ok(agent) => json!({"status": "ok", "agent": agent}).to_string(),
        Err(error) => json!({"status": "error", "error": error.to_string()}).to_string(),
    }
}

pub async fn execute_context_new(ctx: ToolExecutionContext, args: Value) -> String {
    let request = ContextCreateRequest {
        workspace_uuid: ctx.workspace_uuid.clone(),
        title: string_arg(&args, "title").unwrap_or_else(|| "Untitled context".to_string()),
        content: string_arg(&args, "content").unwrap_or_default(),
    };
    match ctx.handles.context.create_context(request).await {
        Ok(context) => json!({"status": "ok", "context": context}).to_string(),
        Err(error) => json!({"status": "error", "error": error.to_string()}).to_string(),
    }
}

pub async fn execute_workflow_new(ctx: ToolExecutionContext, args: Value) -> String {
    let request = WorkflowCreateRequest {
        workspace_uuid: ctx.workspace_uuid.clone(),
        title: string_arg(&args, "title").unwrap_or_else(|| "Untitled workflow".to_string()),
        content: string_arg(&args, "content").unwrap_or_default(),
        metadata: metadata_arg(&args),
    };
    match ctx.handles.storage.create_workflow(request).await {
        Ok(workflow) => json!({"status": "ok", "workflow": workflow}).to_string(),
        Err(error) => json!({"status": "error", "error": error.to_string()}).to_string(),
    }
}

fn string_arg(args: &Value, name: &str) -> Option<String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn string_array_arg(args: &Value, name: &str) -> Vec<String> {
    args.get(name)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn metadata_arg(args: &Value) -> HashMap<String, String> {
    args.get("metadata")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect()
}
