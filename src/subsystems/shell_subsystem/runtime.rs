use crate::bus::{
    Bus, Method, ReplyChannel, Request, Response, StreamChunk, Subsystem, SubsystemName,
};
use crate::subsystems::agent_subsystem::model::{
    AgentApproveRequest, AgentInputRequest, AgentResponse,
};
use crate::subsystems::response_body_as;
use crate::subsystems::shell_subsystem::model::{
    ShellApproveRequest, ShellEvent, ShellInputRequest, ShellSubmitRequest, ShellSubsystem,
};
use anyhow::{Result, anyhow};
use serde_json::json;
use tokio::sync::mpsc;

impl ShellSubsystem {
    pub fn new() -> Self {
        Self {
            active_agent_uuid: String::new(),
        }
    }

    pub fn with_active_agent(active_agent_uuid: impl Into<String>) -> Self {
        Self {
            active_agent_uuid: active_agent_uuid.into(),
        }
    }

    async fn handle_request(&mut self, bus: Bus, req: &Request) -> Response {
        match (req.method, req.path.as_str()) {
            (Method::Get, "snapshot") => match request_agent_snapshot(&bus).await {
                Ok(event) => {
                    self.sync_active_agent(&event);
                    Response::ok(json!(event))
                }
                Err(error) => Response::internal_error(error),
            },
            (Method::Post, "submit") => {
                let request = match response_body_as::<ShellSubmitRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.handle_submit(&bus, request).await {
                    Ok(event) => Response::ok(json!(event)),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "input") => {
                let request = match response_body_as::<ShellInputRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let agent_uuid = match self.resolve_agent_uuid(&bus, request.agent_uuid).await {
                    Ok(agent_uuid) => agent_uuid,
                    Err(error) => return Response::internal_error(error),
                };
                match forward_input(&bus, agent_uuid, request.content).await {
                    Ok(event) => {
                        self.sync_active_agent(&event);
                        Response::ok(json!(event))
                    }
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "approve") => {
                let request = match response_body_as::<ShellApproveRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let agent_uuid = match self.resolve_agent_uuid(&bus, request.agent_uuid).await {
                    Ok(agent_uuid) => agent_uuid,
                    Err(error) => return Response::internal_error(error),
                };
                match forward_approve(&bus, agent_uuid, request.args).await {
                    Ok(event) => Response::ok(json!(event)),
                    Err(error) => Response::internal_error(error),
                }
            }
            _ => Response::not_found(req.path.as_str()),
        }
    }

    async fn handle_submit(
        &mut self,
        bus: &Bus,
        request: ShellSubmitRequest,
    ) -> Result<ShellEvent> {
        let content = request.content.trim().to_string();
        if content.is_empty() {
            return Ok(ShellEvent::Patch {
                correlation_uuid: None,
                text: String::new(),
            });
        }

        if let Some(command) = content.strip_prefix('/') {
            return self
                .handle_command(bus, request.agent_uuid, command.trim())
                .await;
        }

        let agent_uuid = self.resolve_agent_uuid(bus, request.agent_uuid).await?;
        let event = forward_input(bus, agent_uuid, content).await?;
        self.sync_active_agent(&event);
        Ok(event)
    }

    async fn handle_command(
        &mut self,
        bus: &Bus,
        agent_uuid: Option<String>,
        command: &str,
    ) -> Result<ShellEvent> {
        match command {
            "approve" => {
                let agent_uuid = self.resolve_agent_uuid(bus, agent_uuid).await?;
                forward_approve(bus, agent_uuid, "all".to_string()).await
            }
            command if command.starts_with("approve ") => {
                let args = command["approve ".len()..].trim();
                let args = if args.is_empty() { "all" } else { args };
                let agent_uuid = self.resolve_agent_uuid(bus, agent_uuid).await?;
                forward_approve(bus, agent_uuid, args.to_string()).await
            }
            command => Ok(ShellEvent::Patch {
                correlation_uuid: None,
                text: format!("unknown shell command: /{command}"),
            }),
        }
    }

    fn sync_active_agent(&mut self, event: &ShellEvent) {
        if let ShellEvent::Snapshot { snapshot, .. } = event {
            self.active_agent_uuid = snapshot.active_agent_uuid.clone();
        }
    }

    async fn resolve_agent_uuid(
        &mut self,
        bus: &Bus,
        agent_uuid: Option<String>,
    ) -> Result<String> {
        if let Some(agent_uuid) = agent_uuid {
            return Ok(agent_uuid);
        }
        if !self.active_agent_uuid.is_empty() {
            return Ok(self.active_agent_uuid.clone());
        }
        let event = request_agent_snapshot(bus).await?;
        self.sync_active_agent(&event);
        if self.active_agent_uuid.is_empty() {
            return Err(anyhow!("agent subsystem returned no active agent"));
        }
        Ok(self.active_agent_uuid.clone())
    }
}

impl Subsystem for ShellSubsystem {
    fn name(&self) -> SubsystemName {
        SubsystemName::Shell
    }

    fn start(self, bus: Bus) -> mpsc::Sender<Request> {
        let (tx, mut rx) = mpsc::channel::<Request>(64);
        let mut subsystem = self;

        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let response = subsystem.handle_request(bus.clone(), &req).await;
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

async fn request_agent_snapshot(bus: &Bus) -> Result<ShellEvent> {
    let response = bus
        .get(SubsystemName::Agent, SubsystemName::Shell, "snapshot")
        .await
        .map_err(|error| anyhow!("failed to request agent snapshot: {error}"))?;
    agent_event_from_response(response)
}

async fn forward_input(bus: &Bus, agent_uuid: String, content: String) -> Result<ShellEvent> {
    let response = bus
        .post(
            SubsystemName::Agent,
            SubsystemName::Shell,
            "input",
            json!(AgentInputRequest {
                agent_uuid,
                content,
            }),
        )
        .await
        .map_err(|error| anyhow!("failed to forward shell input to agent: {error}"))?;
    agent_event_from_response(response)
}

async fn forward_approve(bus: &Bus, agent_uuid: String, args: String) -> Result<ShellEvent> {
    let response = bus
        .post(
            SubsystemName::Agent,
            SubsystemName::Shell,
            "approve",
            json!(AgentApproveRequest { agent_uuid, args }),
        )
        .await
        .map_err(|error| anyhow!("failed to forward approve to agent: {error}"))?;
    agent_event_from_response(response)
}

fn agent_event_from_response(response: Response) -> Result<ShellEvent> {
    if !response.is_ok() {
        return Err(anyhow!(
            "agent subsystem request failed: {:?}",
            response.body
        ));
    }
    let response = response_body_as::<AgentResponse>(response.body)?;
    Ok(response.event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Subsystem;
    use crate::subsystems::agent_subsystem::model::AgentSubsystem;

    #[tokio::test]
    async fn shell_input_returns_mock_agent_snapshot() {
        let bus = Bus::new();
        let agent = AgentSubsystem::mock();
        let active_agent_uuid = agent.active_agent_uuid.clone();
        let agent_tx = agent.start(bus.clone());
        bus.register(SubsystemName::Agent, agent_tx).await;

        let shell = ShellSubsystem::new();
        let shell_tx = shell.start(bus.clone());
        bus.register(SubsystemName::Shell, shell_tx).await;

        let response = bus
            .post(
                SubsystemName::Shell,
                SubsystemName::Shell,
                "input",
                json!(ShellInputRequest {
                    agent_uuid: None,
                    content: "hello".to_string(),
                }),
            )
            .await
            .unwrap();

        assert!(response.is_ok());
        let event = response_body_as::<ShellEvent>(response.body).unwrap();
        let ShellEvent::Snapshot { snapshot, .. } = event else {
            panic!("expected snapshot");
        };
        assert_eq!(snapshot.active_agent_uuid, active_agent_uuid);
        assert_eq!(snapshot.agents.len(), 1);
        assert_eq!(snapshot.agents[0].messages.len(), 2);
        assert_eq!(snapshot.agents[0].messages[0].role, "user");
        assert_eq!(snapshot.agents[0].messages[0].content, "hello");
        assert_eq!(snapshot.agents[0].messages[1].role, "assistant");
        assert_eq!(snapshot.agents[0].messages[1].content, "hello");
    }
}
