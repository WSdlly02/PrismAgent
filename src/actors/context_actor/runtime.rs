use std::path::PathBuf;

use crate::bus::{
    Bus, Method, ReplyChannel, Request, Response, StreamChunk, Subsystem, SubsystemName,
};
use crate::subsystems::context_subsystem::model::{
    ContextReadRequest, ContextReadResponse, ContextRenderRequest, ContextResolveFailure,
    ContextResolveRequest, ContextResolveResponse, ContextSubsystem, ContextWriteRequest,
};
use crate::subsystems::response_body_as;
use crate::subsystems::storage_subsystem::model::context::Context;
use crate::subsystems::storage_subsystem::model::unit::Unit;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;

impl ContextSubsystem {
    pub fn new() -> Self {
        Self
    }

    async fn handle_request(&self, bus: Bus, req: &Request) -> Response {
        match (req.method, req.path.as_str()) {
            (Method::Get, "list") => match list_contexts(&bus).await {
                Ok(body) => Response::ok(body),
                Err(error) => Response::internal_error(error),
            },
            (Method::Post, "read") => {
                let request = match response_body_as::<ContextReadRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match read_contexts(&bus, request.uuids).await {
                    Ok(response) => Response::ok(json!(response)),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "resolve") => {
                let request = match response_body_as::<ContextResolveRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match resolve_context_inputs(&bus, request).await {
                    Ok(response) => Response::ok(json!(response)),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "render") => {
                let request = match response_body_as::<ContextRenderRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match resolve_context_inputs(
                    &bus,
                    ContextResolveRequest {
                        unit_uuids: request.unit_uuids,
                        context_uuids: request.context_uuids,
                    },
                )
                .await
                {
                    Ok(resolved) => Response::ok(json!({
                        "content": render_context_inputs(&resolved),
                        "failed": resolved.failed,
                    })),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "write") => {
                let request = match response_body_as::<ContextWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match write_contexts(&bus, request.contexts).await {
                    Ok(body) => Response::ok(body),
                    Err(error) => Response::internal_error(error),
                }
            }
            _ => Response::not_found(req.path.as_str()),
        }
    }
}

impl Subsystem for ContextSubsystem {
    fn name(&self) -> SubsystemName {
        SubsystemName::Context
    }

    fn start(self, bus: Bus) -> mpsc::Sender<Request> {
        let (tx, mut rx) = mpsc::channel::<Request>(64);
        let subsystem = std::sync::Arc::new(self);

        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let subsystem = subsystem.clone();
                let bus = bus.clone();
                tokio::spawn(async move {
                    let response = subsystem.handle_request(bus, &req).await;
                    match req.reply {
                        ReplyChannel::Once(tx) => {
                            let _ = tx.send(response);
                        }
                        ReplyChannel::Stream(tx) => {
                            let _ = tx.send(StreamChunk::Delta(response.body)).await;
                            let _ = tx.send(StreamChunk::Done).await;
                        }
                        ReplyChannel::None => {
                            let _ = response;
                        }
                    }
                });
            }
        });

        tx
    }
}

async fn detect_available_skills(bus: &Bus) -> Result<Vec<String>> {
    let response = bus
        .get(
            SubsystemName::Config,
            SubsystemName::Context,
            "global_config_path",
        )
        .await?;
    if !response.is_ok() {
        return Err(anyhow!(
            "Config response missing global config path: {:?}",
            response.body
        ));
    }
    let global_config_path = response
        .body
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Config response missing global config path"))?;
    let global_skills_path = PathBuf::from(global_config_path)
        .join("skills")
        .to_str()
        .map(|s| s.to_string())
        .expect("Not a valid path");

    let response = bus
        .get(
            SubsystemName::Config,
            SubsystemName::Context,
            "workspace_config_path",
        )
        .await?;
    if !response.is_ok() {
        return Err(anyhow!(
            "Config response missing workspace config path: {:?}",
            response.body
        ));
    }
    let workspace_config_path = response
        .body
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Config response missing workspace config path"))?;
    let workspace_skills_path = PathBuf::from(workspace_config_path)
        .join("skills")
        .to_str()
        .map(|s| s.to_string())
        .expect("Not a valid path");

    Ok(vec![global_skills_path, workspace_skills_path])
}

async fn resolve_context_inputs(
    bus: &Bus,
    request: ContextResolveRequest,
) -> Result<ContextResolveResponse> {
    let units_response = if request.unit_uuids.is_empty() {
        StorageUnitsResponse::default()
    } else {
        let response = bus
            .post(
                SubsystemName::Storage,
                SubsystemName::Context,
                "unit/read",
                json!({ "uuids": request.unit_uuids }),
            )
            .await
            .map_err(|e| anyhow!("Failed to request units from storage: {e}"))?;
        if !response.is_ok() {
            return Err(anyhow!("Storage unit/read failed: {:?}", response.body));
        }
        response_body_as::<StorageUnitsResponse>(response.body)?
    };

    let contexts_response = if request.context_uuids.is_empty() {
        StorageContextsResponse::default()
    } else {
        let response = bus
            .post(
                SubsystemName::Storage,
                SubsystemName::Context,
                "context/read",
                json!({ "uuids": request.context_uuids }),
            )
            .await
            .map_err(|e| anyhow!("Failed to request contexts from storage: {e}"))?;
        if !response.is_ok() {
            return Err(anyhow!("Storage context/read failed: {:?}", response.body));
        }
        response_body_as::<StorageContextsResponse>(response.body)?
    };

    let failed = units_response
        .failed
        .into_iter()
        .map(|failure| ContextResolveFailure {
            target: "unit".to_string(),
            uuid: failure.uuid,
            error: failure.error,
        })
        .chain(
            contexts_response
                .failed
                .into_iter()
                .map(|failure| ContextResolveFailure {
                    target: "context".to_string(),
                    uuid: failure.uuid,
                    error: failure.error,
                }),
        )
        .collect();

    Ok(ContextResolveResponse {
        units: units_response.units,
        contexts: contexts_response.contexts,
        failed,
    })
}

async fn write_contexts(bus: &Bus, contexts: Vec<Context>) -> Result<Value> {
    let response = bus
        .post(
            SubsystemName::Storage,
            SubsystemName::Context,
            "context/write",
            json!({ "contexts": contexts }),
        )
        .await
        .map_err(|e| anyhow!("Failed to request context write from storage: {e}"))?;
    if !response.is_ok() {
        return Err(anyhow!("Storage context/write failed: {:?}", response.body));
    }
    Ok(response.body)
}

async fn list_contexts(bus: &Bus) -> Result<Value> {
    let response = bus
        .get(
            SubsystemName::Storage,
            SubsystemName::Context,
            "context/list",
        )
        .await
        .map_err(|e| anyhow!("Failed to request context list from storage: {e}"))?;
    if !response.is_ok() {
        return Err(anyhow!("Storage context/list failed: {:?}", response.body));
    }
    Ok(response.body)
}

async fn read_contexts(bus: &Bus, uuids: Vec<String>) -> Result<ContextReadResponse> {
    let response = bus
        .post(
            SubsystemName::Storage,
            SubsystemName::Context,
            "context/read",
            json!({ "uuids": uuids }),
        )
        .await
        .map_err(|e| anyhow!("Failed to request contexts from storage: {e}"))?;
    if !response.is_ok() {
        return Err(anyhow!("Storage context/read failed: {:?}", response.body));
    }
    let response = response_body_as::<StorageContextsResponse>(response.body)?;
    let failed = response
        .failed
        .into_iter()
        .map(|failure| ContextResolveFailure {
            target: "context".to_string(),
            uuid: failure.uuid,
            error: failure.error,
        })
        .collect();
    Ok(ContextReadResponse {
        contexts: response.contexts,
        failed,
    })
}

fn render_context_inputs(resolved: &ContextResolveResponse) -> String {
    let mut sections = Vec::new();

    if !resolved.contexts.is_empty() {
        let rendered_contexts = resolved
            .contexts
            .iter()
            .map(|context| {
                format!(
                    "## Context: {}\n\n{}\n",
                    context.title.trim(),
                    context.content.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("# Context Documents\n\n{rendered_contexts}"));
    }

    if !resolved.units.is_empty() {
        let rendered_units = resolved
            .units
            .iter()
            .map(|unit| {
                let content = serde_json::to_string_pretty(&unit.content)
                    .unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"));
                format!("## Unit: {}\n\n```json\n{}\n```\n", unit.uuid, content)
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("# Referenced Units\n\n{rendered_units}"));
    }

    sections.join("\n\n")
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct StorageUnitsResponse {
    #[serde(default)]
    units: Vec<Unit>,
    #[serde(default)]
    failed: Vec<StorageObjectFailure>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct StorageContextsResponse {
    #[serde(default)]
    contexts: Vec<Context>,
    #[serde(default)]
    failed: Vec<StorageObjectFailure>,
}

#[derive(Serialize, Deserialize, Debug)]
struct StorageObjectFailure {
    uuid: String,
    error: String,
}
