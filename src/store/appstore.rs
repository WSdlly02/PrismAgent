use crate::model::app::{App, GlobalConfig};
use crate::model::workspace::WorkSpace;
use anyhow::{Context, Result, anyhow};
use directories::BaseDirs;

impl GlobalConfig {
    pub fn read_global_config() -> Result<Self> {
        // ~/.config/prismagent/settings.toml
        let config_path = BaseDirs::new()
            .ok_or_else(|| anyhow!("Failed to determine config directory"))?
            .config_dir()
            .join("prismagent")
            .join("settings.toml");
        let config_data = std::fs::read_to_string(&config_path)
            .context("Failed to read global config as string")?;
        toml::from_str(&config_data).context("Failed to parse global config TOML")
    }
}
impl App {
    pub fn new() -> Result<Self> {
        let global_config = GlobalConfig::read_global_config()?;
        let workspace = WorkSpace::resume_or_init_workspace()?;
        Ok(Self {
            global_config,
            workspace,
        })
    }
}
