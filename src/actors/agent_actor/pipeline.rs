use crate::actors::agent_actor::model::MessageBody;
use crate::actors::agent_actor::model::{
    AgentInferenceOutput, AgentTaskError, AgentTaskOperation, AgentTaskPhase, AgentTaskResult,
    SendMessageRequest, ToolBatchOutput,
};
use crate::actors::agent_actor::runtime::ApprovalMask;
use crate::actors::llm_actor::model::{LlmInferRequest, LlmStreamEvent};
use crate::actors::profile_actor::model::ToolsConfigSection;
use crate::actors::shell_actor::model::WsEvent;
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::tools_actor::model::{ToolApproval, ToolBatchRequest, ToolStreamEvent};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use genai::chat::{ChatMessage, ContentPart, ToolCall, ToolResponse};
use serde_json::json;
use std::collections::HashSet;
use tokio::sync::mpsc;

pub fn input_pipeline(
    mut units: Vec<Unit>,
    message_body: MessageBody,
) -> SubsystemResult<Vec<Unit>> {
    units.push(Unit::from_chat_message(user_message(message_body)?));
    Ok(units)
}

fn user_message(message_body: MessageBody) -> SubsystemResult<ChatMessage> {
    let MessageBody { text, attachments } = message_body;
    if text.trim().is_empty() && attachments.is_empty() {
        return Err(SubsystemError::invalid_input(
            "message must contain text or at least one attachment",
        ));
    }

    let mut parts = Vec::with_capacity(usize::from(!text.trim().is_empty()) + attachments.len());
    if !text.trim().is_empty() {
        parts.push(ContentPart::from_text(text));
    }
    for attachment in attachments {
        if attachment.data.trim().is_empty() {
            return Err(SubsystemError::invalid_input(format!(
                "attachment data must not be empty: {}",
                attachment.filename
            )));
        }
        if attachment.mimetype.trim().is_empty() {
            return Err(SubsystemError::invalid_input(format!(
                "attachment mimetype must not be empty: {}",
                attachment.filename
            )));
        }
        parts.push(ContentPart::from_binary_base64(
            attachment.mimetype,
            attachment.data,
            Some(attachment.filename),
        ));
    }

    Ok(ChatMessage::user(parts))
}

pub async fn run_llm_inference(
    handles: &AppHandles,
    workspace_uuid: String,
    unit_uuids: Vec<String>,
    request: SendMessageRequest,
    profile_name: String,
    inference_uuid: String,
) -> AgentTaskResult<AgentInferenceOutput> {
    let operation = AgentTaskOperation::LlmInference;
    let history_len = unit_uuids.len();
    let units = if unit_uuids.is_empty() {
        Vec::new()
    } else {
        handles
            .storage
            .read_units(&workspace_uuid, unit_uuids)
            .await
            .map_err(|source| AgentTaskError::new(operation, AgentTaskPhase::ReadHistory, source))?
    };
    let mut units = input_pipeline(units, request.message_body)
        .map_err(|source| AgentTaskError::new(operation, AgentTaskPhase::BuildInput, source))?;
    let response = call_llm_with_units(
        handles,
        request.agent_uuid.clone(),
        profile_name,
        inference_uuid,
        units.clone(),
        operation,
    )
    .await?;
    units.push(response.output_unit);
    Ok(AgentInferenceOutput {
        units: units.split_off(history_len),
        is_tool_calls: response.is_tool_calls,
    })
}

pub async fn run_llm_continuation(
    handles: &AppHandles,
    workspace_uuid: String,
    agent_uuid: String,
    unit_uuids: Vec<String>,
    profile_name: String,
    inference_uuid: String,
) -> AgentTaskResult<AgentInferenceOutput> {
    let operation = AgentTaskOperation::LlmContinuation;
    let history_len = unit_uuids.len();
    let mut units = handles
        .storage
        .read_units(&workspace_uuid, unit_uuids)
        .await
        .map_err(|source| AgentTaskError::new(operation, AgentTaskPhase::ReadHistory, source))?;
    let response = call_llm_with_units(
        handles,
        agent_uuid,
        profile_name,
        inference_uuid,
        units.clone(),
        operation,
    )
    .await?;
    units.push(response.output_unit);
    Ok(AgentInferenceOutput {
        units: units.split_off(history_len),
        is_tool_calls: response.is_tool_calls,
    })
}

pub async fn run_tool_batch(
    handles: &AppHandles,
    workspace_uuid: String,
    agent_uuid: String,
    profile_name: String,
    job_uuid: String,
    tool_calls: Vec<ToolCall>,
    approval_mask: ApprovalMask,
    denied_reason: String,
) -> AgentTaskResult<ToolBatchOutput> {
    let operation = AgentTaskOperation::ToolBatch;
    let workspace = handles
        .workspace
        .get(&workspace_uuid)
        .await
        .map_err(|source| AgentTaskError::new(operation, AgentTaskPhase::LoadWorkspace, source))?;
    let (tool_stream_tx, mut tool_stream_rx) = mpsc::channel::<ToolStreamEvent>(64);
    let shell = handles.shell.clone();
    let stream_agent_uuid = agent_uuid.clone();
    let tool_stream_forwarder = tokio::spawn(async move {
        while let Some(event) = tool_stream_rx.recv().await {
            let text = match event {
                ToolStreamEvent::Started { tool_count } => {
                    format!("tool batch started: {tool_count} call(s)")
                }
                ToolStreamEvent::ToolStarted { index, name } => {
                    format!("tool {index} started: {name}")
                }
                ToolStreamEvent::ToolFinished { index, name } => {
                    format!("tool {index} finished: {name}")
                }
                ToolStreamEvent::Finished => "tool batch finished".to_string(),
            };
            let _ =
                shell.emit_agent_event(stream_agent_uuid.clone(), WsEvent::StreamDelta { text });
        }
    });
    let tools_config = handles
        .profile
        .tools(&profile_name)
        .await
        .map_err(|source| {
            AgentTaskError::new(operation, AgentTaskPhase::LoadToolsConfig, source)
        })?;
    let continue_loop = approval_mask.approves_all(tool_calls.len());
    let approvals = tool_calls
        .iter()
        .enumerate()
        .map(|(index, tool_call)| {
            let available = tool_is_available(&tools_config, &tool_call.fn_name);
            let approved = approval_mask.approves(index) && available;
            ToolApproval {
                approved,
                reason: if !available {
                    Some(format!(
                        "tool is not available in profile: {}",
                        tool_call.fn_name
                    ))
                } else {
                    (!approved).then(|| denied_reason.clone())
                },
            }
        })
        .collect();
    let tool_response = handles
        .tools
        .dispatch_batch(ToolBatchRequest {
            job_uuid: job_uuid.clone(),
            workspace_uuid: workspace_uuid.clone(),
            caller_agent_uuid: agent_uuid.clone(),
            workspace_path: workspace.path,
            tool_calls,
            approvals,
            stream_tx: tool_stream_tx,
        })
        .await
        .map_err(|source| AgentTaskError::new(operation, AgentTaskPhase::DispatchTools, source))?;
    let _ = tool_stream_forwarder.await;
    Ok(ToolBatchOutput {
        units: tool_response.output_units,
        continue_loop,
    })
}

pub fn clone_tool_calls(unit: &Unit) -> Vec<ToolCall> {
    unit.content
        .content
        .tool_calls()
        .into_iter()
        .cloned()
        .collect()
}

/// Checks that genai-parsed tool calls are structurally sound.
///
/// When an LLM stream is cut off mid-JSON, genai may produce a `ToolCall`
/// with malformed arguments such as `Null` or a raw string. Tool arguments
/// must be a JSON object; no-argument tools should use `{}`.
pub fn tool_calls_sound(tool_calls: &[ToolCall]) -> bool {
    if tool_calls.is_empty() {
        return false;
    }
    tool_calls.iter().all(|tc| {
        !tc.call_id.trim().is_empty()
            && !tc.fn_name.trim().is_empty()
            && tc.fn_arguments.is_object()
    })
}

pub fn tool_batch_is_auto_approved(config: &ToolsConfigSection, tool_calls: &[ToolCall]) -> bool {
    if tool_calls.is_empty() {
        return false;
    }
    auto_approval_mask(config, tool_calls) == ApprovalMask::all_for(tool_calls.len()).bits()
}

pub fn auto_approval_mask(config: &ToolsConfigSection, tool_calls: &[ToolCall]) -> u64 {
    if tool_calls.is_empty() {
        return 0;
    }
    let auto_all = config.auto_approve.iter().any(|name| name == "*");
    let auto = config.auto_approve.iter().cloned().collect::<HashSet<_>>();
    tool_calls
        .iter()
        .enumerate()
        .fold(0, |mask, (index, tool_call)| {
            let is_available = tool_is_available(config, &tool_call.fn_name);
            let is_auto = config.yolo || auto_all || auto.contains(&tool_call.fn_name);
            if index < 64 && is_available && is_auto {
                mask | (1u64 << index)
            } else {
                mask
            }
        })
}

pub fn tool_response_units(tool_calls: &[ToolCall], status: &str, reason: &str) -> Vec<Unit> {
    tool_calls
        .iter()
        .map(|tool_call| {
            let content = json!({
                "status": status,
                "reason": reason,
            })
            .to_string();
            Unit::from_chat_message(ChatMessage::from(ToolResponse::from_tool_call(
                tool_call, content,
            )))
        })
        .collect()
}

async fn call_llm_with_units(
    handles: &AppHandles,
    agent_uuid: String,
    profile_name: String,
    inference_uuid: String,
    units: Vec<Unit>,
    operation: AgentTaskOperation,
) -> AgentTaskResult<crate::actors::llm_actor::model::LlmInferResponse> {
    let model = handles
        .profile
        .model_config(&profile_name)
        .await
        .map_err(|source| {
            AgentTaskError::new(operation, AgentTaskPhase::LoadModelConfig, source)
        })?;
    let tools_config = handles
        .profile
        .tools(profile_name)
        .await
        .map_err(|source| {
            AgentTaskError::new(operation, AgentTaskPhase::LoadToolsConfig, source)
        })?;
    let tools = if tools_config.available_tools.is_empty() {
        Vec::new()
    } else {
        handles
            .tools
            .list(Some(tools_config.available_tools.clone()))
            .await
            .map_err(|source| {
                AgentTaskError::new(operation, AgentTaskPhase::ResolveTools, source)
            })?
    };
    let (stream_tx, mut stream_rx) = mpsc::channel::<LlmStreamEvent>(64);
    let shell = handles.shell.clone();
    let stream_forwarder = tokio::spawn(async move {
        while let Some(event) = stream_rx.recv().await {
            match event {
                LlmStreamEvent::TextDelta { text } => {
                    let _ =
                        shell.emit_agent_event(agent_uuid.clone(), WsEvent::StreamDelta { text });
                }
                LlmStreamEvent::ReasoningDelta { text } => {
                    let _ = shell
                        .emit_agent_event(agent_uuid.clone(), WsEvent::ReasoningDelta { text });
                }
                LlmStreamEvent::Started
                | LlmStreamEvent::ToolCallDelta { .. }
                | LlmStreamEvent::Finished => {}
            }
        }
    });
    let response = handles
        .llm
        .infer(LlmInferRequest {
            inference_uuid,
            model,
            units,
            tools,
            stream_tx,
        })
        .await
        .map_err(|source| {
            AgentTaskError::new(operation, AgentTaskPhase::ProviderInference, source)
        })?;
    let _ = stream_forwarder.await;
    Ok(response)
}

fn tool_is_available(config: &ToolsConfigSection, tool_name: &str) -> bool {
    config.available_tools.iter().any(|name| name == "*")
        || config.available_tools.iter().any(|name| name == tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::agent_actor::model::Attachment;
    use serde_json::json;

    #[test]
    fn user_message_supports_attachment_only_input() {
        let message = user_message(MessageBody {
            text: String::new(),
            attachments: vec![Attachment {
                data: "aGVsbG8=".to_string(),
                filename: "note.txt".to_string(),
                mimetype: "text/plain".to_string(),
            }],
        })
        .unwrap();

        assert!(message.content.contains_binary());
        assert!(!message.content.contains_text());
    }

    #[test]
    fn auto_approval_mask_tracks_tool_call_indices() {
        let config = ToolsConfigSection {
            yolo: false,
            available_tools: vec!["*".to_string()],
            auto_approve: vec!["safe_tool".to_string()],
        };
        let tool_calls = vec![
            tool_call("manual_a"),
            tool_call("safe_tool"),
            tool_call("manual_b"),
        ];

        assert_eq!(auto_approval_mask(&config, &tool_calls), 0b010);
        assert!(!tool_batch_is_auto_approved(&config, &tool_calls));
    }

    #[test]
    fn yolo_auto_approves_all_available_tools() {
        let config = ToolsConfigSection {
            yolo: true,
            available_tools: vec!["safe_tool".to_string(), "other_tool".to_string()],
            auto_approve: Vec::new(),
        };
        let tool_calls = vec![tool_call("safe_tool"), tool_call("other_tool")];

        assert_eq!(auto_approval_mask(&config, &tool_calls), 0b11);
        assert!(tool_batch_is_auto_approved(&config, &tool_calls));
    }

    fn tool_call(name: &str) -> ToolCall {
        ToolCall {
            call_id: format!("{name}-call"),
            fn_name: name.to_string(),
            fn_arguments: json!({}),
            thought_signatures: None,
        }
    }

    #[test]
    fn sound_rejects_null_arguments() {
        let tcs = vec![ToolCall {
            call_id: "c1".into(),
            fn_name: "fs_read".into(),
            fn_arguments: serde_json::Value::Null,
            thought_signatures: None,
        }];
        assert!(!tool_calls_sound(&tcs));
    }

    #[test]
    fn sound_rejects_empty_fn_name() {
        let tcs = vec![ToolCall {
            call_id: "c1".into(),
            fn_name: String::new(),
            fn_arguments: json!({"path": "/tmp"}),
            thought_signatures: None,
        }];
        assert!(!tool_calls_sound(&tcs));
    }

    #[test]
    fn sound_rejects_empty_call_id() {
        let tcs = vec![ToolCall {
            call_id: String::new(),
            fn_name: "fs_read".into(),
            fn_arguments: json!({"path": "/tmp"}),
            thought_signatures: None,
        }];
        assert!(!tool_calls_sound(&tcs));
    }

    #[test]
    fn sound_rejects_string_arguments() {
        let tcs = vec![ToolCall {
            call_id: "c1".into(),
            fn_name: "fs_read".into(),
            fn_arguments: json!("{\"path\": \"src/actors/workflow_actor/runtime.rs\""),
            thought_signatures: None,
        }];
        assert!(!tool_calls_sound(&tcs));
    }

    #[test]
    fn sound_rejects_empty_list() {
        assert!(!tool_calls_sound(&[]));
    }

    #[test]
    fn sound_accepts_valid_tool_call() {
        let tcs = vec![tool_call("fs_read")];
        assert!(tool_calls_sound(&tcs));
    }
}
