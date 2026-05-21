use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

pub(super) struct ConfigSubsystem {
    pub(super) global_config: GlobalConfig,
    pub(super) global_config_path: PathBuf,
    pub(super) workspace_config: WorkspaceConfig,
    pub(super) workspace_config_path: PathBuf,
}

/// ~/.config/prismagent/config.toml
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(super) struct GlobalConfig {
    /// current_provider = "deepseek"
    pub(super) current_provider: String,
    /// `[providers.deepseek]`
    ///
    /// api_key = "xxx"
    ///
    /// model = "deepseek-v4-flash"
    pub(super) providers: HashMap<String, ProviderConfig>,

    /// `[tools.tinyfish]`
    ///
    /// api_key = "xxx"
    pub(super) tools: HashMap<String, Value>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(super) struct ProviderConfig {
    pub(super) api_key: String,
    pub(super) model: String,
}
/// 最终模型相关配置，初始化时加载，不是配置文件
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub(super) struct FinalModelConfig {
    pub(super) final_api_key: String,
    pub(super) final_model: String,
}

/// $PWD/.prismagent
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub workspace: WorkspaceConfigSection,
    pub runtime: RuntimeConfigSection,
    pub concurrency: ConcurrencyConfigSection,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfigSection {
    pub state_version: u32,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfigSection {
    pub entry_mode: EntryMode, // "manual_resume"
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ConcurrencyConfigSection {
    pub lock_scope: LockScope, // "workspace"
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub enum EntryMode {
    #[serde(rename = "manual_resume")]
    ManualResume,
    #[serde(rename = "auto_start")]
    AutoStart,
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
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
