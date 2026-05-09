use crate::model::run::Run;
use crate::store::workspacestore::{WorkSpace, atomic_write_file};
use anyhow::{Result, anyhow};

impl WorkSpace {
    pub fn list_runs(&self) -> Result<Vec<Run>> {
        // $PWD/.prismagent/runs
        let runs_dir = &self.root.join("runs");
        if !runs_dir.is_dir() {
            return Err(anyhow!("{} is not a directory", &runs_dir.display()));
        }
        let mut result: Vec<Run> = Vec::new();
        for entry in std::fs::read_dir(runs_dir)? {
            // $PWD/.prismagent/runs/{run_id}
            let path = entry?.path();
            if !path.is_dir() {
                return Err(anyhow!("Expected directory for run: {}", path.display()));
            }
            let metadata_path = path.join("metadata.json");
            let run = std::fs::read(metadata_path)
                .map_err(|e| anyhow!("Failed to read unit store: {}", e))
                .and_then(|data| {
                    serde_json::from_slice(&data)
                        .map_err(|e| anyhow!("Failed to parse unit store JSON: {}", e))
                })?;
            result.push(run);
        }
        Ok(result)
    }
    pub fn create_run(&self, run_id: &str) -> Result<()> {
        // $PWD/.prismagent/runs/{run_id}/metadata.json
        let run_dir = &self.root.join("runs").join(run_id);
        if run_dir.exists() {
            return Err(anyhow!("Run already exists: {}", run_dir.display()));
        }
        std::fs::create_dir_all(run_dir)?;
        let metadata_path = run_dir.join("metadata.json");
        let run = Run {}; // TODO: initialize run metadata!!!
        let data = serde_json::to_vec(&run)
            .map_err(|e| anyhow!("Failed to serialize run to JSON: {}", e))?;
        atomic_write_file(&metadata_path, &data)?;
        Ok(())
    }
}
