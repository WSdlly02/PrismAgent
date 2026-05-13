use crate::model::workspace::{DEFAULT_WORKSPACE_CONFIG, WorkSpace, WorkspaceConfig};
use anyhow::{Context, Result, anyhow};
use std::path::PathBuf;
use toml::from_str;
impl WorkSpace {
    pub fn resume_or_init_workspace() -> Result<Self> {
        // Check if workspace config exists at $PWD/.prismagent/config.toml
        let root = std::env::current_dir()?.join(".prismagent");
        let workspace_config_path = &root.join("config.toml");
        if workspace_config_path.is_file() {
            let workspace_config: WorkspaceConfig = {
                let data_str = std::fs::read_to_string(workspace_config_path)
                    .context("Failed to read workspace config as string")?;
                from_str(&data_str).context("Failed to parse workspace config TOML")?
            };
            // Make atoms directory
            std::fs::create_dir_all(root.join("atoms"))?;
            // Make runs directory
            std::fs::create_dir_all(root.join("runs"))?;
            return Ok(Self {
                root,
                workspace_config,
            });
        }
        // Initialize new workspace
        atomic_create_file(workspace_config_path, DEFAULT_WORKSPACE_CONFIG.as_bytes())?;
        WorkSpace::resume_or_init_workspace()
    }
}

pub(crate) fn atomic_create_file(dst: &PathBuf, data: &[u8]) -> Result<()> {
    if dst.exists() {
        return Err(anyhow!("File already exists: {}", dst.display()));
    }
    std::fs::create_dir_all(
        dst.parent()
            .ok_or_else(|| anyhow!("Invalid path: no parent directory"))?,
    )?;
    let tmp_dst = dst.with_extension("tmp");
    std::fs::write(&tmp_dst, data)?;
    std::fs::rename(tmp_dst, dst)?;
    Ok(())
}
pub(crate) fn atomic_replace_file(dst: &PathBuf, data: &[u8]) -> Result<()> {
    if !dst.exists() {
        return Err(anyhow!("File does not exist: {}", dst.display()));
    }
    let tmp_dst = dst.with_extension("tmp");
    std::fs::write(&tmp_dst, data)?;
    std::fs::rename(tmp_dst, dst)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::workspace::{EntryMode, LockScope};

    #[test]
    fn default_workspace_config_uses_manual_resume_and_run_lock_scope() {
        let config: WorkspaceConfig =
            toml::from_str(DEFAULT_WORKSPACE_CONFIG).expect("parse default config");

        assert_eq!(config.workspace.state_version, 1);
        assert_eq!(config.runtime.entry_mode, EntryMode::ManualResume);
        assert_eq!(config.concurrency.lock_scope, LockScope::Run);
    }

    #[test]
    fn atomic_write_file_is_create_only() {
        let path = std::env::temp_dir()
            .join("prismagent-tests")
            .join(uuid::Uuid::now_v7().to_string())
            .join("file.txt");

        atomic_create_file(&path, b"first").expect("first write");
        assert!(atomic_create_file(&path, b"second").is_err());
        assert_eq!(std::fs::read(&path).expect("read file"), b"first");
    }
}
