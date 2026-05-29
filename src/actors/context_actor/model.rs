use crate::subsystems::storage_subsystem::model::context::Context;
use crate::subsystems::storage_subsystem::model::unit::Unit;
use serde::{Deserialize, Serialize};

pub struct SystemPrompt {
    pub role: String,
    pub skills: String,
    pub content: String,
}

pub struct ContextSubsystem;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ContextReadRequest {
    #[serde(default)]
    pub uuids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ContextResolveRequest {
    #[serde(default)]
    pub unit_uuids: Vec<String>,
    #[serde(default)]
    pub context_uuids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ContextRenderRequest {
    #[serde(default)]
    pub unit_uuids: Vec<String>,
    #[serde(default)]
    pub context_uuids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContextWriteRequest {
    pub contexts: Vec<Context>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContextReadResponse {
    pub contexts: Vec<Context>,
    pub failed: Vec<ContextResolveFailure>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContextResolveResponse {
    pub units: Vec<Unit>,
    pub contexts: Vec<Context>,
    pub failed: Vec<ContextResolveFailure>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContextResolveFailure {
    pub target: String,
    pub uuid: String,
    pub error: String,
}
