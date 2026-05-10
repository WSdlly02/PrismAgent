use crate::model::run::{Run, RunLock, RunMetadata, RunStatus};
use crate::model::workspace::WorkSpace;
use crate::store::workspacestore::atomic_write_file;
use anyhow::{Result, anyhow};
use chrono::Utc;

impl WorkSpace {
    pub fn list_runs(&self) -> Result<Vec<Run>> {
        // $PWD/.prismagent/runs
        let runs_dir = &self.root.join("runs");
        if !runs_dir.is_dir() {
            return Err(anyhow!("{} is not a directory", &runs_dir.display()));
        }
        let mut result: Vec<Run> = Vec::new();
        for entry in std::fs::read_dir(runs_dir)? {
            // $PWD/.prismagent/runs/<run-id>
            let path = entry?.path();
            if !path.is_dir() {
                return Err(anyhow!("Expected directory for run: {}", path.display()));
            };
            let run_metadata = std::fs::read(path.join("metadata.json"))
                .map_err(|e| anyhow!("Failed to read unit store: {}", e))
                .and_then(|data| {
                    serde_json::from_slice(&data)
                        .map_err(|e| anyhow!("Failed to parse unit store JSON: {}", e))
                })?;
            let run_lock: Option<RunLock> = {
                let lock_path = path.join("run.lock");
                if lock_path.is_file() {
                    Some(
                        std::fs::read(lock_path)
                            .map_err(|e| anyhow!("Failed to read run lock: {}", e))
                            .and_then(|data| {
                                serde_json::from_slice(&data)
                                    .map_err(|e| anyhow!("Failed to parse run lock JSON: {}", e))
                            })?,
                    )
                } else {
                    None
                }
            };
            result.push(Run {
                root: path,
                run_metadata,
                run_lock,
            });
        }
        Ok(result)
    }
    pub fn create_run_and_resume(&self, title: &str) -> Result<Run> {
        // Create new run indicates creating lock file and metadata.json
        // $PWD/.prismagent/runs/{run_id}/metadata.json
        let run_id = format!("run-{}", Utc::now().timestamp());
        let run_dir = &self.root.join("runs").join(&run_id);
        if run_dir.exists() {
            return Err(anyhow!("Run already exists: {}", run_dir.display()));
        }
        std::fs::create_dir_all(run_dir)?;
        let timestamp = Utc::now().timestamp();
        let run_metadata = RunMetadata {
            run_id,
            title: title.to_string(),
            status: RunStatus::Active,
            root_agent: "agent-0".to_string(),
            created_at: timestamp,
            updated_at: timestamp,
        };
        let run_lock = RunLock {
            pid: std::process::id(),
            owner: whoami::username()?,
            locked_at: timestamp,
            hostname: whoami::hostname()?,
            note: None,
        };

        let data = serde_json::to_vec(&run_metadata)
            .map_err(|e| anyhow!("Failed to serialize run to JSON: {}", e))?;
        atomic_write_file(&run_dir.join("metadata.json"), &data)?;
        let data = serde_json::to_vec(&run_lock)
            .map_err(|e| anyhow!("Failed to serialize run lock to JSON: {}", e))?;
        atomic_write_file(&run_dir.join("run.lock"), &data)?;

        Ok(Run {
            root: run_dir.clone(),
            run_metadata,
            run_lock: Some(run_lock),
        })
    }
    pub fn resume_run(&self, run_id: &str) -> Result<Run> {
        // Resume run indicates checking lock file and metadata.json
        // the lock file must not exist
        let run_dir = &self.root.join("runs").join(run_id);
        if !run_dir.is_dir() {
            return Err(anyhow!("Run does not exist: {}", run_dir.display()));
        }
        let run_metadata = std::fs::read(run_dir.join("metadata.json"))
            .map_err(|e| anyhow!("Failed to read unit store: {}", e))
            .and_then(|data| {
                serde_json::from_slice(&data)
                    .map_err(|e| anyhow!("Failed to parse unit store JSON: {}", e))
            })?;
        if run_dir.join("run.lock").is_file() {
            return Err(anyhow!(
                "Run is locked, cannot resume: {}",
                run_dir.display()
            ));
        }
        Ok(Run {
            root: run_dir.clone(),
            run_metadata,
            run_lock: None,
        })
    }
    pub fn release_run_lock(&self, run_id: &str) -> Result<()> {
        let run_dir = &self.root.join("runs").join(run_id);
        if !run_dir.is_dir() {
            return Err(anyhow!("Run does not exist: {}", run_dir.display()));
        }
        let lock_path = run_dir.join("run.lock");
        if lock_path.is_file() {
            std::fs::remove_file(lock_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::workspace::{
        ConcurrencyConfigSection, EntryMode, LockScope, RuntimeConfigSection, WorkspaceConfig,
        WorkspaceConfigSection,
    };
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_workspace() -> WorkSpace {
        let root = std::env::temp_dir()
            .join("prismagent-tests")
            .join(Uuid::new_v4().to_string())
            .join(".prismagent");
        WorkSpace {
            root: PathBuf::from(root),
            workspace_config: WorkspaceConfig {
                workspace: WorkspaceConfigSection { state_version: 1 },
                runtime: RuntimeConfigSection {
                    entry_mode: EntryMode::ManualResume,
                },
                concurrency: ConcurrencyConfigSection {
                    lock_scope: LockScope::Run,
                },
            },
        }
    }

    #[test]
    fn create_run_writes_metadata_and_lock() {
        let workspace = test_workspace();
        let run = workspace
            .create_run_and_resume("state model")
            .expect("create run");

        assert_eq!(run.run_metadata.title, "state model");
        assert!(run.run_lock.is_some());
        assert!(run.root.join("metadata.json").is_file());
        assert!(run.root.join("run.lock").is_file());
    }

    #[test]
    fn locked_run_cannot_be_resumed_until_released() {
        let workspace = test_workspace();
        let run = workspace
            .create_run_and_resume("locked run")
            .expect("create run");
        let run_id = run.run_metadata.run_id.clone();

        assert!(workspace.resume_run(&run_id).is_err());

        workspace.release_run_lock(&run_id).expect("release lock");
        let resumed = workspace.resume_run(&run_id).expect("resume unlocked run");

        assert_eq!(resumed.run_metadata.run_id, run_id);
        assert!(resumed.run_lock.is_none());
    }

    #[test]
    fn list_runs_reports_lock_state() {
        let workspace = test_workspace();
        let run = workspace
            .create_run_and_resume("listed run")
            .expect("create run");

        let runs = workspace.list_runs().expect("list runs");

        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_metadata.run_id, run.run_metadata.run_id);
        assert!(runs[0].run_lock.is_some());
    }
}
