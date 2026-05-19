use std::collections::HashMap;

use crate::model::workspace::WorkSpace;
use serde::{Deserialize, Serialize};
pub struct App {
    pub global_config: GlobalConfig,
    pub workspace: WorkSpace,
    pub model_config: ModelConfig,
}
/// ~/.config/prismagent/settings.toml
#[derive(Serialize, Deserialize, Debug)]
pub struct GlobalConfig {
    /// current_provider = "deepseek"
    pub current_provider: String,
    /// `[providers.deepseek]`
    ///
    /// api_key = "xxx"
    ///
    /// model = "deepseek-v4-flash"
    pub providers: HashMap<String, ProviderConfig>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct ProviderConfig {
    pub api_key: String,
    pub model: String,
}
/// 最终模型相关配置，初始化时加载，不是配置文件
pub struct ModelConfig {
    pub final_api_key: String,
    pub final_model: String,
}
