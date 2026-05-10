use crate::model::workspace::WorkSpace;
use serde::{Deserialize, Serialize};
pub struct App {
    pub global_config: GlobalConfig,
    pub workspace: WorkSpace,
}
// ~/.config/prismagent/settings.toml
#[derive(Serialize, Deserialize, Debug)]
pub struct GlobalConfig {
    pub env: EnvConfigSection,
    // A placeholder for future use
    // pub placeholder: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct EnvConfigSection {
    #[serde(rename = "PRISMAGENT_PROVIDER")]
    pub provider: String,
    #[serde(rename = "PRISMAGENT_BASE_URL")]
    pub base_url: String,
    #[serde(rename = "PRISMAGENT_API_KEY")]
    pub api_key: String,
    #[serde(rename = "PRISMAGENT_MODEL")]
    pub model: String,
}
