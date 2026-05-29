use serde::{Deserialize, Serialize};

pub struct ShellSubsystem {
    pub active_agent_uuid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellInputRequest {
    pub content: String,
    pub agent_uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSubmitRequest {
    pub content: String,
    pub agent_uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellApproveRequest {
    pub args: String,
    pub agent_uuid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShellEvent {
    Patch {
        correlation_uuid: Option<String>,
        text: String,
    },
    Snapshot {
        correlation_uuid: Option<String>,
        snapshot: ShellSnapshot,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellSnapshot {
    pub active_agent_uuid: String,
    pub agents: Vec<ShellAgentSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellAgentSnapshot {
    pub agent_uuid: String,
    pub name: String,
    pub messages: Vec<ShellMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellMessage {
    pub role: String,
    pub content: String,
}
