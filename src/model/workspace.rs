use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// $PWD/.prismagent
#[derive(Serialize, Deserialize, Debug)]
pub struct WorkSpace {
    pub root: PathBuf,                     // $PWD/.prismagent
    pub workspace_config: WorkspaceConfig, // 从 $PWD/.prismagent/config.toml 读取
}
#[derive(Serialize, Deserialize, Debug)]
pub struct WorkspaceConfig {
    pub workspace: WorkspaceConfigSection,
    pub runtime: RuntimeConfigSection,
    pub concurrency: ConcurrencyConfigSection,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct WorkspaceConfigSection {
    pub state_version: u32,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct RuntimeConfigSection {
    pub entry_mode: EntryMode, // "manual_resume"
}
#[derive(Serialize, Deserialize, Debug)]
pub struct ConcurrencyConfigSection {
    pub lock_scope: LockScope, // "workspace"
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum EntryMode {
    #[serde(rename = "manual_resume")]
    ManualResume,
    #[serde(rename = "auto_start")]
    AutoStart,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum LockScope {
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "run")]
    Run,
}
pub const DEFAULT_WORKSPACE_CONFIG: &str = r#"[workspace]
state_version = 1

[runtime]
entry_mode = "manual_resume"

[concurrency]
lock_scope = "run"
"#;
