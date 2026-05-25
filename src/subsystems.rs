use anyhow::{Context, Result};
use serde_json::Value;

pub(crate) mod agent_subsystem;
pub(crate) mod config_subsystem;
pub(crate) mod context_subsystem;
pub(crate) mod llm_subsystem;
pub(crate) mod storage_subsystem;
pub(crate) mod tools_subsystem;
pub(crate) mod workflow_subsystem;

pub(crate) fn response_body_as<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).context("Failed to deserialize config response body")
}
