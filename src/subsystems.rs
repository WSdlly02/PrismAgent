use anyhow::{Context, Result};
use serde_json::Value;

pub mod agent_subsystem;
pub mod config_subsystem;
pub mod context_subsystem;
pub mod llm_subsystem;
pub mod shell_subsystem;
pub mod storage_subsystem;
pub mod tools_subsystem;
pub mod workflow_subsystem;

pub(crate) fn response_body_as<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).context("Failed to deserialize config response body")
}
