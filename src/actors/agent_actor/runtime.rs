use crate::actors::agent_actor::model::{
    AGENT_ACTOR, AgentActor, AgentEvent, AgentHandle, AgentMsg, AgentSnapshot, AgentState,
    AgentStatus, AgentUnit, ApproveRequest, SendMessageRequest,
};
use crate::error::{SubsystemError, SubsystemResult};
use std::collections::HashMap;
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

impl AgentActor {
    pub fn mock(rx: mpsc::Receiver<AgentMsg>, agent_uuid: String, agent_name: String) -> Self {
        let (events, _) = broadcast::channel(128);
        let mut agents = HashMap::new();
        agents.insert(
            agent_uuid.clone(),
            AgentState {
                uuid: agent_uuid,
                name: agent_name,
                units: Vec::new(),
                status: AgentStatus::Idle,
                events,
            },
        );
        Self { rx, agents }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                AgentMsg::Snapshot { agent_uuid, reply } => {
                    let _ = reply.send(self.snapshot(&agent_uuid));
                }
                AgentMsg::Subscribe { agent_uuid, reply } => {
                    let _ = reply.send(self.subscribe(&agent_uuid));
                }
                AgentMsg::SendMessage { request, reply } => {
                    let _ = reply.send(self.send_message(request));
                }
                AgentMsg::ApproveRequest { request, reply } => {
                    let _ = reply.send(self.approve_request(request));
                }
                AgentMsg::Cancel { agent_uuid, reply } => {
                    let _ = reply.send(self.cancel(&agent_uuid));
                }
            }
        }
    }

    fn snapshot(&self, agent_uuid: &str) -> SubsystemResult<AgentSnapshot> {
        let agent = self.agent(agent_uuid)?;
        Ok(AgentSnapshot {
            agent_uuid: agent.uuid.clone(),
            agent_name: agent.name.clone(),
            units: agent.units.clone(),
            status: agent.status.clone(),
        })
    }

    fn subscribe(&self, agent_uuid: &str) -> SubsystemResult<broadcast::Receiver<AgentEvent>> {
        Ok(self.agent(agent_uuid)?.events.subscribe())
    }

    fn send_message(&mut self, request: SendMessageRequest) -> SubsystemResult<()> {
        if request.message_body.text.trim().is_empty() {
            return Err(SubsystemError::invalid_input(
                "message text must not be empty",
            ));
        }
        let agent = self.agent_mut(&request.agent_uuid)?;
        set_status(agent, AgentStatus::Running);
        append_unit(agent, "user", request.message_body.text.clone());

        // Mock response. The real implementation will call ContextHandle and LlmHandle here.
        append_unit(agent, "assistant", request.message_body.text);
        set_status(agent, AgentStatus::Idle);
        Ok(())
    }

    fn approve_request(&mut self, request: ApproveRequest) -> SubsystemResult<()> {
        let agent = self.agent_mut(&request.agent_uuid)?;
        append_unit(
            agent,
            "system",
            format!(
                "approval {}: {}",
                request.request_uuid,
                if request.approved {
                    "approved"
                } else {
                    "rejected"
                }
            ),
        );
        set_status(agent, AgentStatus::Idle);
        Ok(())
    }

    fn cancel(&mut self, agent_uuid: &str) -> SubsystemResult<()> {
        let agent = self.agent_mut(agent_uuid)?;
        set_status(agent, AgentStatus::Idle);
        Ok(())
    }

    fn agent(&self, agent_uuid: &str) -> SubsystemResult<&AgentState> {
        self.agents
            .get(agent_uuid)
            .ok_or_else(|| SubsystemError::not_found("agent", agent_uuid))
    }

    fn agent_mut(&mut self, agent_uuid: &str) -> SubsystemResult<&mut AgentState> {
        self.agents
            .get_mut(agent_uuid)
            .ok_or_else(|| SubsystemError::not_found("agent", agent_uuid))
    }
}

impl AgentHandle {
    pub async fn snapshot(&self, agent_uuid: impl Into<String>) -> SubsystemResult<AgentSnapshot> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(AgentMsg::Snapshot {
                agent_uuid: agent_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?
    }

    pub async fn subscribe(
        &self,
        agent_uuid: impl Into<String>,
    ) -> SubsystemResult<broadcast::Receiver<AgentEvent>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(AgentMsg::Subscribe {
                agent_uuid: agent_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?
    }

    pub async fn send_message(&self, request: SendMessageRequest) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(AgentMsg::SendMessage {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?
    }

    pub async fn approve_request(&self, request: ApproveRequest) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(AgentMsg::ApproveRequest {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?
    }

    pub async fn cancel(&self, agent_uuid: impl Into<String>) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(AgentMsg::Cancel {
                agent_uuid: agent_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?
    }
}

fn append_unit(agent: &mut AgentState, role: &str, content: String) {
    let unit = AgentUnit {
        unit_uuid: Uuid::now_v7().to_string(),
        role: role.to_string(),
        content,
        created_at: chrono::Utc::now().timestamp(),
    };
    agent.units.push(unit.clone());
    let _ = agent.events.send(AgentEvent::UnitAppend { unit });
}

fn set_status(agent: &mut AgentState, status: AgentStatus) {
    agent.status = status.clone();
    let _ = agent.events.send(AgentEvent::StatusChanged { status });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::agent_actor::model::{AgentHandle, AgentMsg, MessageBody};

    #[tokio::test]
    async fn send_message_appends_units_and_broadcasts_events() {
        let (tx, rx) = mpsc::channel::<AgentMsg>(8);
        AgentActor::mock(rx, "agent".to_string(), "agent-0".to_string()).spawn();
        let handle = AgentHandle { tx };
        let mut events = handle.subscribe("agent").await.unwrap();

        handle
            .send_message(SendMessageRequest {
                agent_uuid: "agent".to_string(),
                message_body: MessageBody {
                    text: "hello".to_string(),
                    attachments: Vec::new(),
                },
            })
            .await
            .unwrap();

        let snapshot = handle.snapshot("agent").await.unwrap();
        assert_eq!(snapshot.units.len(), 2);
        assert_eq!(snapshot.units[0].role, "user");
        assert_eq!(snapshot.units[1].role, "assistant");
        assert!(matches!(
            events.recv().await.unwrap(),
            AgentEvent::StatusChanged {
                status: AgentStatus::Running
            }
        ));
        assert!(matches!(
            events.recv().await.unwrap(),
            AgentEvent::UnitAppend { .. }
        ));
    }
}
