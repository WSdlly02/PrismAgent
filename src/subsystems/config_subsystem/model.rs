use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ConfigSubsystem {
    pub global_config: GlobalConfig,
    pub global_config_path: PathBuf,
    pub workspace_config: WorkspaceConfig,
    pub workspace_config_path: PathBuf,
}

/// ~/.config/prismagent/config.toml
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct GlobalConfig {
    /// current_provider = "deepseek"
    pub current_provider: String,
    /// `[providers.deepseek]`
    ///
    /// api_key = "xxx"
    ///
    /// model = "deepseek-v4-flash"
    pub providers: HashMap<String, ProviderConfig>,

    /// `[tools.tinyfish]`
    ///
    /// api_key = "xxx"
    pub tools: HashMap<String, Value>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    pub api_key: String,
    pub model: String,
}
/// 最终模型相关配置，初始化时加载，不是配置文件
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FinalModelConfig {
    pub final_api_key: String,
    pub final_model: String,
}

/// $PWD/.prismagent
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub workspace: WorkspaceConfigSection,
    pub runtime: RuntimeConfigSection,
    pub permissions: PermissionsConfigSection,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfigSection {
    pub state_version: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfigSection {
    pub entry_mode: EntryMode, // "manual_load"
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub enum EntryMode {
    #[serde(rename = "manual_load")]
    ManualLoad,
    #[serde(rename = "auto_load_latest")]
    AutoLoadLatest,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PermissionsConfigSection {
    pub tools: ToolsConfigSection,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ToolsConfigSection {
    pub auto_approve: bool,
}

pub const DEFAULT_WORKSPACE_CONFIG: &str = r#"[workspace]
state_version = 1

[runtime]
entry_mode = "manual_load"

[permissions]
tools.auto_approve = false
"#;
