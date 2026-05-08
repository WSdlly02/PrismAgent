use crate::model::agent::Agent;
use anyhow::{Result, anyhow};
use std::path::PathBuf;

pub fn read_agent_store(agent_src: &PathBuf) -> Result<Agent> {
    std::fs::read(agent_src)
        .map_err(|e| anyhow!("Failed to read agent store: {}", e))
        .and_then(|data| {
            serde_json::from_slice(&data)
                .map_err(|e| anyhow!("Failed to parse agent store JSON: {}", e))
        })
}
pub fn write_agent_store(agent_dst: &PathBuf, agent: &Agent) -> Result<()> {
    std::fs::create_dir_all(
        agent_dst
            .parent()
            .ok_or_else(|| anyhow!("Invalid agent store path: no parent directory"))?,
    )?;
    let data = serde_json::to_vec(agent)
        .map_err(|e| anyhow!("Failed to serialize agent to JSON: {}", e))?;
    std::fs::write(agent_dst, data).map_err(|e| anyhow!("Failed to write agent store: {}", e))
}
