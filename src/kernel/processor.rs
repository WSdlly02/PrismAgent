use crate::kernel::pipeline::unit_with_content;
use crate::model::asyncioinstance::{AsyncIoInstance, InstanceSignal};
use crate::model::event::InstanceToKernelEvent;
use crate::model::unit::{Unit, UnitRole, UnitVisibility};
use crate::tools::registry::{dispatch_tool, tools_registry};
use anyhow::{Result, anyhow};
use genai::Client;
use genai::chat::{ChatMessage, ChatRequest, ToolResponse};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

pub fn spawn_llm_instance(
    client: Client,
    model: String,
    run_root: PathBuf,
    request_uuid: String,
    run_uuid: String,
    agent_uuid: String,
    instance: AsyncIoInstance,
) {
    tokio::spawn(async move {
        let AsyncIoInstance {
            uuid: instance_uuid,
            mut stdin,
            mut signal_rx,
            kernel_tx,
            ..
        } = instance;

        let units = tokio::select! {
            units = stdin.recv() => {
                let Some(units) = units else {
                    send_instance_error(
                        &kernel_tx,
                        request_uuid,
                        run_uuid,
                        agent_uuid,
                        instance_uuid,
                        "AsyncIoInstance closed before receiving input.".to_string(),
                    ).await;
                    return;
                };
                units
            }
            signal = signal_rx.recv() => {
                let message = match signal {
                    Some(InstanceSignal::Terminate) => "AsyncIoInstance terminated before receiving input.",
                    Some(InstanceSignal::Interrupt) => "AsyncIoInstance interrupted before receiving input.",
                    Some(InstanceSignal::Approve { .. }) => "AsyncIoInstance received approval before receiving input.",
                    None => "AsyncIoInstance signal channel closed before receiving input.",
                };
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    message.to_string(),
                ).await;
                return;
            }
        };

        match run_llm(&client, &model, &run_root, units).await {
            Ok((units, is_tool_calls)) => {
                let _ = kernel_tx
                    .send(InstanceToKernelEvent {
                        correlation_uuid: Some(request_uuid),
                        run_uuid,
                        agent_uuid,
                        asyncioinstance_uuid: instance_uuid,
                        units,
                        is_tool_calls,
                    })
                    .await;
            }
            Err(error) => {
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    format!("LLM instance failed: {error}"),
                )
                .await;
            }
        }
    });
}

pub fn spawn_tool_instance(
    run_root: PathBuf,
    request_uuid: String,
    run_uuid: String,
    agent_uuid: String,
    instance: AsyncIoInstance,
) {
    tokio::spawn(async move {
        let AsyncIoInstance {
            uuid: instance_uuid,
            mut stdin,
            mut signal_rx,
            kernel_tx,
            ..
        } = instance;

        let units = tokio::select! {
            units = stdin.recv() => {
                let Some(units) = units else {
                    send_instance_error(
                        &kernel_tx,
                        request_uuid,
                        run_uuid,
                        agent_uuid,
                        instance_uuid,
                        "Tool instance closed before receiving input.".to_string(),
                    ).await;
                    return;
                };
                units
            }
            signal = signal_rx.recv() => {
                let message = match signal {
                    Some(InstanceSignal::Terminate) => "Tool instance terminated before receiving input.",
                    Some(InstanceSignal::Interrupt) => "Tool instance interrupted before receiving input.",
                    Some(InstanceSignal::Approve { .. }) => "Tool instance received approval before receiving input.",
                    None => "Tool instance signal channel closed before receiving input.",
                };
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    message.to_string(),
                ).await;
                return;
            }
        };

        let args = match signal_rx.recv().await {
            Some(InstanceSignal::Approve { args }) => args,
            Some(InstanceSignal::Terminate) => {
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    "Tool instance terminated before approval.".to_string(),
                )
                .await;
                return;
            }
            Some(InstanceSignal::Interrupt) => {
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    "Tool instance interrupted before approval.".to_string(),
                )
                .await;
                return;
            }
            None => {
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    "Tool instance signal channel closed before approval.".to_string(),
                )
                .await;
                return;
            }
        };

        match run_tools(&run_root, units, &args).await {
            Ok(units) => {
                let _ = kernel_tx
                    .send(InstanceToKernelEvent {
                        correlation_uuid: Some(request_uuid),
                        run_uuid,
                        agent_uuid,
                        asyncioinstance_uuid: instance_uuid,
                        units,
                        is_tool_calls: false,
                    })
                    .await;
            }
            Err(error) => {
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    format!("Tool instance failed: {error}"),
                )
                .await;
            }
        }
    });
}

async fn run_llm(
    client: &Client,
    model: &str,
    run_root: &Path,
    mut units: Vec<Unit>,
) -> Result<(Vec<Unit>, bool)> {
    let messages = convert_units_to_chat_messages(run_root, &units)?;
    let req = ChatRequest::new(messages).with_tools(tools_registry());
    let res = client.exec_chat(model, req, None).await?;
    let reasoning = res.reasoning_content.clone();

    if res.tool_calls().is_empty() {
        let text = res.first_text().unwrap_or("").to_string();
        let message = ChatMessage::assistant(text.clone()).with_reasoning_content(reasoning);
        units.push(unit_from_chat_message(
            UnitRole::Assistant,
            &message,
            if text.is_empty() {
                "(empty assistant response)".to_string()
            } else {
                text
            },
        )?);
        Ok((units, false))
    } else {
        let tool_calls = res.into_tool_calls();
        let preview = tool_calls
            .iter()
            .enumerate()
            .map(|(index, call)| format!("[{}] {} {}", index + 1, call.fn_name, call.fn_arguments))
            .collect::<Vec<_>>()
            .join("\n");
        let message = ChatMessage::from(tool_calls).with_reasoning_content(reasoning);
        units.push(unit_from_chat_message(
            UnitRole::Assistant,
            &message,
            format!("Tool calls requested:\n{preview}"),
        )?);
        Ok((units, true))
    }
}

fn unit_from_chat_message(role: UnitRole, message: &ChatMessage, preview: String) -> Result<Unit> {
    let mut metadata = HashMap::from([("preview".to_string(), preview)]);
    metadata.insert(
        "message_format".to_string(),
        "genai.chat_message".to_string(),
    );
    let content = serde_json::to_string(message)
        .map_err(|e| anyhow!("Failed to serialize ChatMessage: {e}"))?;
    Ok(unit_with_content(
        role,
        UnitVisibility::Public,
        None,
        content,
        metadata,
    ))
}

async fn run_tools(run_root: &Path, mut units: Vec<Unit>, approve_args: &str) -> Result<Vec<Unit>> {
    let messages = convert_units_to_chat_messages(run_root, &units)?;
    let Some(last_message) = messages.last() else {
        return Err(anyhow!("Tool instance received an empty unit chain"));
    };
    let tool_calls = last_message
        .content
        .tool_calls()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    if tool_calls.is_empty() {
        return Err(anyhow!(
            "Tool instance expected the last message to contain tool calls"
        ));
    }

    let approved_indices = approved_tool_indices(approve_args, tool_calls.len())?;
    for (index, tool_call) in tool_calls.iter().enumerate() {
        let content = if approved_indices.contains(&index) {
            dispatch_tool(run_root, tool_call).await
        } else {
            json!({
                "status": "denied",
                "tool": tool_call.fn_name,
                "reason": "not approved",
            })
            .to_string()
        };
        let response = ToolResponse::from_tool_call(tool_call, content.clone());
        let message = ChatMessage::from(response);
        units.push(unit_from_chat_message(
            UnitRole::Tool,
            &message,
            format!("tool {} -> {content}", tool_call.fn_name),
        )?);
    }

    Ok(units)
}

fn approved_tool_indices(args: &str, total: usize) -> Result<Vec<usize>> {
    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
        return Ok((0..total).collect());
    }

    let mut indices = Vec::new();
    for part in trimmed.split_whitespace() {
        let value = part
            .parse::<usize>()
            .map_err(|_| anyhow!("Invalid approve argument: {part}"))?;
        if value == 0 || value > total {
            return Err(anyhow!(
                "Approve argument {value} is outside the range 1..={total}"
            ));
        }
        let index = value - 1;
        if !indices.contains(&index) {
            indices.push(index);
        }
    }
    Ok(indices)
}

fn convert_units_to_chat_messages(run_root: &Path, units: &[Unit]) -> Result<Vec<ChatMessage>> {
    units
        .iter()
        .map(|unit| unit_to_chat_message(run_root, unit))
        .collect()
}

fn unit_to_chat_message(run_root: &Path, unit: &Unit) -> Result<ChatMessage> {
    let content = unit_content(run_root, unit)?;
    if unit.metadata.get("message_format").map(String::as_str) == Some("genai.chat_message")
        && let Ok(message) = serde_json::from_str::<ChatMessage>(&content)
    {
        return Ok(message);
    }

    Ok(match unit.role {
        UnitRole::System => ChatMessage::system(content),
        UnitRole::User => ChatMessage::user(content),
        UnitRole::Assistant => ChatMessage::assistant(content),
        UnitRole::Tool => ChatMessage::tool(content),
    })
}

fn unit_content(run_root: &Path, unit: &Unit) -> Result<String> {
    if let Some(content) = unit.metadata.get("content") {
        return Ok(content.clone());
    }
    if unit.atom_hash == crate::kernel::pipeline::PENDING_ATOM_HASH {
        return Err(anyhow!(
            "Pending unit {} does not contain content",
            unit.uuid
        ));
    }

    let workspace_root = run_root
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow!("Invalid run root: {}", run_root.display()))?;
    let atom_path = workspace_root
        .join("atoms")
        .join(&unit.atom_hash[0..2])
        .join(&unit.atom_hash[2..]);
    std::fs::read_to_string(&atom_path)
        .map_err(|e| anyhow!("Failed to read atom file {:?}: {}", atom_path, e))
}

async fn send_instance_error(
    kernel_tx: &mpsc::Sender<InstanceToKernelEvent>,
    request_uuid: String,
    run_uuid: String,
    agent_uuid: String,
    instance_uuid: String,
    message: String,
) {
    let _ = kernel_tx
        .send(InstanceToKernelEvent {
            correlation_uuid: Some(request_uuid),
            run_uuid,
            agent_uuid,
            asyncioinstance_uuid: instance_uuid,
            units: vec![unit_with_content(
                UnitRole::System,
                UnitVisibility::Public,
                None,
                message,
                HashMap::new(),
            )],
            is_tool_calls: false,
        })
        .await;
}
