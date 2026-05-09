use crate::{
    model::agent::Agent,
    store::workspacestore::{WorkSpace, atomic_write_file},
};
use anyhow::{Result, anyhow};
use std::path::PathBuf;

impl WorkSpace {
    pub fn read_agent_store(agent_src: &PathBuf) -> Result<Agent> {
        std::fs::read(agent_src)
            .map_err(|e| anyhow!("Failed to read agent store: {}", e))
            .and_then(|data| {
                serde_json::from_slice(&data)
                    .map_err(|e| anyhow!("Failed to parse agent store JSON: {}", e))
            })
    }
    pub fn write_agent_store(agent_dst: &PathBuf, agent: &Agent) -> Result<()> {
        let data = serde_json::to_vec(agent)
            .map_err(|e| anyhow!("Failed to serialize agent to JSON: {}", e))?;
        atomic_write_file(agent_dst, &data)
    }
}
