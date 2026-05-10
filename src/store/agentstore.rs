use crate::model::agent::Agent;
use crate::model::run::Run;
use crate::store::workspacestore::atomic_write_file;
use anyhow::{Result, anyhow};
use std::path::PathBuf;

impl Run {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        agent::Agent,
        run::{RunLock, RunMetadata, RunStatus},
    };
    use std::{collections::HashMap, path::PathBuf};
    use uuid::Uuid;

    fn test_run() -> Run {
        let root = std::env::temp_dir()
            .join("prismagent-tests")
            .join(Uuid::new_v4().to_string())
            .join(".prismagent")
            .join("runs")
            .join("run-test");
        Run {
            root: PathBuf::from(root),
            run_metadata: RunMetadata {
                run_id: "run-test".to_string(),
                title: "test".to_string(),
                status: RunStatus::Active,
                root_agent: "agent-0".to_string(),
                created_at: 1,
                updated_at: 1,
            },
            run_lock: Some(RunLock {
                pid: 1,
                owner: "test".to_string(),
                locked_at: 1,
                hostname: "localhost".to_string(),
                note: None,
            }),
        }
    }

    fn agent() -> Agent {
        Agent {
            name: "agent-0".to_string(),
            unit_chain: vec![
                "unit-system-0".to_string(),
                "unit-user-1".to_string(),
                "unit-assistant-2".to_string(),
            ],
            unit_head: "unit-assistant-2".to_string(),
            children_agents: vec!["agent-1".to_string()],
            snapshots: HashMap::from([(
                "snapshot-before-tool".to_string(),
                vec!["unit-system-0".to_string(), "unit-user-1".to_string()],
            )]),
        }
    }

    #[test]
    fn agent_store_round_trips_chain_and_inline_snapshots() {
        let run = test_run();
        let path = run.root.join("agent-0.json");
        let expected = agent();

        Run::write_agent_store(&path, &expected).expect("write agent");
        let actual = Run::read_agent_store(&path).expect("read agent");

        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.unit_chain, expected.unit_chain);
        assert_eq!(actual.unit_head, expected.unit_head);
        assert_eq!(actual.children_agents, expected.children_agents);
        assert_eq!(
            actual.snapshots.get("snapshot-before-tool"),
            expected.snapshots.get("snapshot-before-tool")
        );
    }

    #[test]
    fn agent_store_is_create_only() {
        let run = test_run();
        let path = run.root.join("agent-0.json");
        let expected = agent();

        Run::write_agent_store(&path, &expected).expect("first write");
        assert!(Run::write_agent_store(&path, &expected).is_err());
    }
}
