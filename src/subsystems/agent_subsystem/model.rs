use crate::subsystems::shell_subsystem::model::ShellEvent;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub struct AgentSubsystem {
    pub agents: HashMap<String, MockAgentState>,
    pub active_agent_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockAgentState {
    pub agent_uuid: String,
    pub name: String,
    pub messages: Vec<MockAgentMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockAgentMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInputRequest {
    pub agent_uuid: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentApproveRequest {
    pub agent_uuid: String,
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub event: ShellEvent,
}
