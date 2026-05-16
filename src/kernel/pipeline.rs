use crate::model::agent::Agent;
use crate::model::run::Run;
use crate::model::unit::{Unit, UnitKind, UnitRole, UnitScope, UnitVisibility};
use crate::model::workspace::WorkSpace;
use crate::store::workspacestore::atomic_replace_file;
use anyhow::{Result, anyhow};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use uuid::Uuid;

/// 64 个 0，表示一个 pending 的 atom hash，只有在 output_pipeline 真正提交时才会被 materialize 成真正的 atom hash
pub const PENDING_ATOM_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

pub fn input_pipeline(
    run: &Run,
    agent_uuid: &str,
    request_uuid: &str,
    content: &str,
) -> Result<Vec<Unit>> {
    let agent = Run::read_agent_store(&agent_path(run, agent_uuid))?;
    let mut units: Vec<Unit> = read_agent_units(run, &agent)?;
    // 追加一个 pending 的用户输入 Unit，等待 output_pipeline 真正提交时被 materialize
    units.push(unit_with_content(
        UnitKind::UserInput,
        UnitRole::User,
        UnitVisibility::Public,
        Some(agent_uuid),
        content.to_string(),
        HashMap::from([("request_uuid".to_string(), request_uuid.to_string())]),
    ));
    Ok(units)
}

pub fn output_pipeline(
    workspace: &WorkSpace,
    run: &mut Run,
    agent_uuid: &str,
    mut units: Vec<Unit>,
) -> Result<Vec<Unit>> {
    let agent_path = agent_path(run, agent_uuid);
    let old_agent_data = std::fs::read(&agent_path)
        .map_err(|e| anyhow!("Failed to read agent store {:?}: {}", agent_path, e))?;
    let mut agent: Agent = serde_json::from_slice(&old_agent_data)
        .map_err(|e| anyhow!("Failed to parse agent store {:?}: {}", agent_path, e))?;
    let known_units = agent.unit_chain.iter().cloned().collect::<HashSet<_>>();
    let mut changed = false;

    for unit in &mut units {
        materialize_atom(workspace, unit)?;
        if known_units.contains(&unit.uuid) {
            continue;
        }

        let unit_path = unit_path(run, &unit.uuid);
        if !unit_path.is_file() {
            Run::write_unit_store(&unit_path, unit)?;
        }
        agent.unit_chain.push(unit.uuid.clone());
        agent.unit_head = unit.uuid.clone();
        changed = true;
    }

    if changed {
        Run::replace_agent_store(&agent_path, &old_agent_data, &agent)?;
        replace_run_metadata_updated_at(run, Utc::now().timestamp())?;
    }

    Ok(units)
}

pub fn unit_with_content(
    kind: UnitKind,
    role: UnitRole,
    visibility: UnitVisibility,
    agent_uuid: Option<&str>,
    content: String,
    mut metadata: HashMap<String, String>,
) -> Unit {
    if let Some(agent_uuid) = agent_uuid {
        metadata.insert("agent_uuid".to_string(), agent_uuid.to_string());
    }
    metadata.insert("content".to_string(), content);
    Unit {
        uuid: Uuid::now_v7().to_string(),
        atom_hash: PENDING_ATOM_HASH.to_string(),
        kind,
        role,
        scope: UnitScope::Agent,
        visibility,
        metadata,
        created_at: Utc::now().timestamp(),
    }
}

fn read_agent_units(run: &Run, agent: &Agent) -> Result<Vec<Unit>> {
    let mut units = Vec::with_capacity(agent.unit_chain.len());
    for unit_uuid in &agent.unit_chain {
        units.push(Run::read_unit_store(&unit_path(run, unit_uuid))?);
    }
    Ok(units)
}

fn materialize_atom(workspace: &WorkSpace, unit: &mut Unit) -> Result<()> {
    if unit.atom_hash != PENDING_ATOM_HASH {
        return Ok(());
    }

    let atom_data = unit
        .metadata
        .get("content")
        .or_else(|| unit.metadata.get("preview"))
        .map(|content| content.as_bytes().to_vec())
        .unwrap_or_else(|| serde_json::to_vec(&unit.metadata).unwrap_or_default());

    if atom_data.is_empty() {
        return Err(anyhow!(
            "cannot materialize empty atom for unit {}",
            unit.uuid
        ));
    }

    unit.atom_hash = workspace.write_atom_store(&atom_data)?;
    Ok(())
}

fn replace_run_metadata_updated_at(run: &mut Run, updated_at: i64) -> Result<()> {
    let metadata_path = run.root.join("metadata.json");
    let old_data = serde_json::to_vec(&run.run_metadata)
        .map_err(|e| anyhow!("Failed to serialize old run metadata to JSON: {}", e))?;
    run.run_metadata.updated_at = updated_at;
    let new_data = serde_json::to_vec(&run.run_metadata)
        .map_err(|e| anyhow!("Failed to serialize new run metadata to JSON: {}", e))?;
    atomic_replace_file(&metadata_path, &old_data, &new_data)
}

fn agent_path(run: &Run, agent_uuid: &str) -> PathBuf {
    run.root.join("agents").join(format!("{agent_uuid}.json"))
}

fn unit_path(run: &Run, unit_uuid: &str) -> PathBuf {
    run.root.join("units").join(format!("{unit_uuid}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::agent::Agent;
    use crate::model::run::{RunLock, RunMetadata, RunStatus};
    use crate::model::workspace::{
        ConcurrencyConfigSection, EntryMode, LockScope, RuntimeConfigSection, WorkspaceConfig,
        WorkspaceConfigSection,
    };

    fn test_workspace_and_run() -> (WorkSpace, Run) {
        let root = std::env::temp_dir()
            .join("prismagent-tests")
            .join(Uuid::now_v7().to_string())
            .join(".prismagent");
        let workspace = WorkSpace {
            root: root.clone(),
            workspace_config: WorkspaceConfig {
                workspace: WorkspaceConfigSection { state_version: 1 },
                runtime: RuntimeConfigSection {
                    entry_mode: EntryMode::ManualResume,
                },
                concurrency: ConcurrencyConfigSection {
                    lock_scope: LockScope::Run,
                },
            },
        };
        let run = Run {
            root: root
                .join("runs")
                .join("018f0000-0000-7000-8000-000000000001"),
            run_metadata: RunMetadata {
                uuid: "018f0000-0000-7000-8000-000000000001".to_string(),
                title: "test".to_string(),
                status: RunStatus::Active,
                root_agent_uuid: "018f0000-0000-7000-8000-000000000002".to_string(),
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
        };
        std::fs::create_dir_all(run.root.join("agents")).expect("agents dir");
        std::fs::create_dir_all(run.root.join("units")).expect("units dir");
        std::fs::create_dir_all(workspace.root.join("atoms")).expect("atoms dir");
        let metadata = serde_json::to_vec(&run.run_metadata).expect("metadata json");
        std::fs::write(run.root.join("metadata.json"), metadata).expect("metadata write");
        let agent = Agent {
            uuid: run.run_metadata.root_agent_uuid.clone(),
            name: "root".to_string(),
            unit_chain: Vec::new(),
            unit_head: String::new(),
            children_agents: Vec::new(),
            snapshots: HashMap::new(),
        };
        Run::write_agent_store(&agent_path(&run, &agent.uuid), &agent).expect("agent write");
        (workspace, run)
    }

    #[test]
    fn input_pipeline_appends_pending_user_unit() {
        let (_workspace, run) = test_workspace_and_run();
        let units = input_pipeline(
            &run,
            &run.run_metadata.root_agent_uuid,
            "request-1",
            "hello",
        )
        .expect("input pipeline");

        assert_eq!(units.len(), 1);
        assert_eq!(units[0].kind, UnitKind::UserInput);
        assert_eq!(units[0].atom_hash, PENDING_ATOM_HASH);
        assert_eq!(units[0].metadata.get("content"), Some(&"hello".to_string()));
    }

    #[test]
    fn output_pipeline_commits_units_and_updates_agent_chain() {
        let (workspace, mut run) = test_workspace_and_run();
        let mut units = input_pipeline(
            &run,
            &run.run_metadata.root_agent_uuid,
            "request-1",
            "hello",
        )
        .expect("input pipeline");
        units.push(unit_with_content(
            UnitKind::GenericResult,
            UnitRole::Assistant,
            UnitVisibility::Public,
            Some(&run.run_metadata.root_agent_uuid),
            "world".to_string(),
            HashMap::new(),
        ));

        let agent_uuid = run.run_metadata.root_agent_uuid.clone();
        let committed =
            output_pipeline(&workspace, &mut run, &agent_uuid, units).expect("output pipeline");

        assert_eq!(committed.len(), 2);
        assert_ne!(committed[0].atom_hash, PENDING_ATOM_HASH);
        assert!(
            run.root
                .join("units")
                .join(format!("{}.json", committed[0].uuid))
                .is_file()
        );
        let agent =
            Run::read_agent_store(&agent_path(&run, &run.run_metadata.root_agent_uuid)).unwrap();
        assert_eq!(
            agent.unit_chain,
            vec![committed[0].uuid.clone(), committed[1].uuid.clone()]
        );
        assert_eq!(agent.unit_head, committed[1].uuid);
    }
}
