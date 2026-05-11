use crate::model::run::Run;
use crate::model::unit::Unit;
use crate::store::workspacestore::atomic_create_file;
use anyhow::{Result, anyhow};
use std::path::PathBuf;

// const UNIT_STORE_DIR: &str = ".prismagent/runs/{run-id}/units";
// 单元路径是传入的函数参数

impl Run {
    pub fn read_unit_store(unit_src: &PathBuf) -> Result<Unit> {
        std::fs::read(unit_src)
            .map_err(|e| anyhow!("Failed to read unit store: {}", e))
            .and_then(|data| {
                serde_json::from_slice(&data)
                    .map_err(|e| anyhow!("Failed to parse unit store JSON: {}", e))
            })
    }
    pub fn write_unit_store(unit_dst: &PathBuf, unit: &Unit) -> Result<()> {
        let data = serde_json::to_vec(unit)
            .map_err(|e| anyhow!("Failed to serialize unit to JSON: {}", e))?;
        atomic_create_file(unit_dst, &data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        run::{RunLock, RunMetadata, RunStatus},
        unit::{UnitKind, UnitRole, UnitScope, UnitVisibility},
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

    fn unit() -> Unit {
        Unit {
            uuid: "unit-1".to_string(),
            atom_hash: "a".repeat(64),
            kind: UnitKind::UserInput,
            role: UnitRole::User,
            scope: UnitScope::Agent,
            visibility: UnitVisibility::Public,
            metadata: HashMap::from([("agent".to_string(), "agent-0".to_string())]),
            created_at: 1,
        }
    }

    #[test]
    fn unit_store_round_trips_unit_json() {
        let run = test_run();
        let path = run.root.join("units").join("unit-1.json");
        let expected = unit();

        Run::write_unit_store(&path, &expected).expect("write unit");
        let actual = Run::read_unit_store(&path).expect("read unit");

        assert_eq!(actual.uuid, expected.uuid);
        assert_eq!(actual.atom_hash, expected.atom_hash);
        assert_eq!(actual.kind, expected.kind);
        assert_eq!(actual.role, expected.role);
        assert_eq!(actual.scope, expected.scope);
        assert_eq!(actual.visibility, expected.visibility);
        assert_eq!(actual.metadata.get("agent"), Some(&"agent-0".to_string()));
    }

    #[test]
    fn unit_store_is_create_only() {
        let run = test_run();
        let path = run.root.join("units").join("unit-1.json");
        let expected = unit();

        Run::write_unit_store(&path, &expected).expect("first write");
        assert!(Run::write_unit_store(&path, &expected).is_err());
    }
}
