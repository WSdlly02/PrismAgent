use crate::bus::{
    Bus, Method, ReplyChannel, Request, Response, StreamChunk, Subsystem, SubsystemName,
};
use crate::subsystems::agent_subsystem::model::{
    AgentApproveRequest, AgentInputRequest, AgentResponse, AgentSubsystem, MockAgentMessage,
    MockAgentState,
};
use crate::subsystems::response_body_as;
use crate::subsystems::shell_subsystem::model::{
    ShellAgentSnapshot, ShellEvent, ShellMessage, ShellSnapshot,
};
use serde_json::json;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

const DEFAULT_AGENT_NAME: &str = "agent-0";

impl AgentSubsystem {
    pub fn mock() -> Self {
        let active_agent_uuid = Uuid::now_v7().to_string();
        let mut agents = HashMap::new();
        agents.insert(
            active_agent_uuid.clone(),
            MockAgentState {
                agent_uuid: active_agent_uuid.clone(),
                name: DEFAULT_AGENT_NAME.to_string(),
                messages: Vec::new(),
            },
        );
        Self {
            agents,
            active_agent_uuid,
        }
    }

    fn handle_request(&mut self, req: &Request) -> Response {
        match (req.method, req.path.as_str()) {
            (Method::Get, "snapshot") => Response::ok(json!(AgentResponse {
                event: ShellEvent::Snapshot {
                    correlation_uuid: Some(req.id.clone()),
                    snapshot: self.snapshot(),
                },
            })),
            (Method::Post, "input") => {
                let request = match response_body_as::<AgentInputRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.handle_input(&req.id, request) {
                    Ok(event) => Response::ok(json!(AgentResponse { event })),
                    Err(error) => Response::bad_request(error),
                }
            }
            (Method::Post, "approve") => {
                let request = match response_body_as::<AgentApproveRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.handle_approve(&req.id, request) {
                    Ok(event) => Response::ok(json!(AgentResponse { event })),
                    Err(error) => Response::bad_request(error),
                }
            }
            _ => Response::not_found(req.path.as_str()),
        }
    }

    fn handle_input(
        &mut self,
        correlation_uuid: &str,
        request: AgentInputRequest,
    ) -> Result<ShellEvent, String> {
        let agent = self
            .agents
            .get_mut(&request.agent_uuid)
            .ok_or_else(|| format!("agent not found: {}", request.agent_uuid))?;
        agent.messages.push(MockAgentMessage {
            role: "user".to_string(),
            content: request.content.clone(),
        });
        agent.messages.push(MockAgentMessage {
            role: "assistant".to_string(),
            content: request.content,
        });
        Ok(ShellEvent::Snapshot {
            correlation_uuid: Some(correlation_uuid.to_string()),
            snapshot: self.snapshot(),
        })
    }

    fn handle_approve(
        &mut self,
        correlation_uuid: &str,
        request: AgentApproveRequest,
    ) -> Result<ShellEvent, String> {
        if !self.agents.contains_key(&request.agent_uuid) {
            return Err(format!("agent not found: {}", request.agent_uuid));
        }
        Ok(ShellEvent::Patch {
            correlation_uuid: Some(correlation_uuid.to_string()),
            text: format!("mock approve accepted: {}", request.args),
        })
    }

    fn snapshot(&self) -> ShellSnapshot {
        let mut agents = self
            .agents
            .values()
            .map(|agent| ShellAgentSnapshot {
                agent_uuid: agent.agent_uuid.clone(),
                name: agent.name.clone(),
                messages: agent
                    .messages
                    .iter()
                    .map(|message| ShellMessage {
                        role: message.role.clone(),
                        content: message.content.clone(),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.name.cmp(&right.name));

        ShellSnapshot {
            active_agent_uuid: self.active_agent_uuid.clone(),
            agents,
        }
    }
}

impl Subsystem for AgentSubsystem {
    fn name(&self) -> SubsystemName {
        SubsystemName::Agent
    }

    fn start(self, _bus: Bus) -> mpsc::Sender<Request> {
        let (tx, mut rx) = mpsc::channel::<Request>(64);
        let mut subsystem = self;

        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let response = subsystem.handle_request(&req);
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
            }
        });

        tx
    }
}
