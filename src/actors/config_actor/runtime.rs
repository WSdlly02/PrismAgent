use crate::actors::config_actor::model::{
    CONFIG_ACTOR, ConfigActor, ConfigHandle, ConfigMsg, CurrentProvider, FinalModelConfig,
};
use crate::error::{SubsystemError, SubsystemResult};
use directories::BaseDirs;
use serde_json::Value;
use tokio::sync::mpsc;

impl ConfigActor {
    pub fn load(rx: mpsc::Receiver<ConfigMsg>) -> SubsystemResult<Self> {
        let global_config_path = default_global_config_path()?;
        let global_config = read_toml_file(&global_config_path)?;

        let workspace_config_path = default_workspace_config_path()?;
        let workspace_config = read_toml_file(&workspace_config_path)?;

        Ok(Self {
            rx,
            global_config,
            global_config_path,
            workspace_config,
            workspace_config_path,
        })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ConfigMsg::GetGlobalConfig { reply } => {
                    let _ = reply.send(Ok(self.global_config.clone()));
                }
                ConfigMsg::GetWorkspaceConfig { reply } => {
                    let _ = reply.send(Ok(self.workspace_config.clone()));
                }
                ConfigMsg::GetCurrentProvider { reply } => {
                    let _ = reply.send(self.current_provider());
                }
                ConfigMsg::GetCurrentModelConfig { reply } => {
                    let _ = reply.send(self.current_model_config());
                }
                ConfigMsg::GetGlobalConfigPath { reply } => {
                    let _ = reply.send(Ok(self.global_config_path.clone()));
                }
                ConfigMsg::GetWorkspaceConfigPath { reply } => {
                    let _ = reply.send(Ok(self.workspace_config_path.clone()));
                }
                ConfigMsg::GetToolConfig { name, reply } => {
                    let _ = reply.send(self.tool_config(&name));
                }
            }
        }
    }

    fn current_provider(&self) -> SubsystemResult<CurrentProvider> {
        let provider = self
            .global_config
            .providers
            .get(&self.global_config.current_provider)
            .cloned()
            .ok_or_else(|| {
                SubsystemError::not_found("provider", self.global_config.current_provider.clone())
            })?;

        Ok(CurrentProvider {
            name: self.global_config.current_provider.clone(),
            provider,
        })
    }

    fn current_model_config(&self) -> SubsystemResult<FinalModelConfig> {
        let provider = self.current_provider()?.provider;
        Ok(FinalModelConfig {
            final_api_key: provider.api_key,
            final_model: provider.model,
        })
    }

    fn tool_config(&self, name: &str) -> SubsystemResult<Value> {
        self.global_config
            .tools
            .get(name)
            .cloned()
            .ok_or_else(|| SubsystemError::not_found("tool_config", name.to_string()))
    }
}

impl ConfigHandle {
    pub async fn global_config(
        &self,
    ) -> SubsystemResult<crate::actors::config_actor::model::GlobalConfig> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ConfigMsg::GetGlobalConfig { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?
    }

    pub async fn workspace_config(
        &self,
    ) -> SubsystemResult<crate::actors::config_actor::model::WorkspaceConfig> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ConfigMsg::GetWorkspaceConfig { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?
    }

    pub async fn current_provider(&self) -> SubsystemResult<CurrentProvider> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ConfigMsg::GetCurrentProvider { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?
    }

    pub async fn current_model_config(&self) -> SubsystemResult<FinalModelConfig> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ConfigMsg::GetCurrentModelConfig { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?
    }

    pub async fn global_config_path(&self) -> SubsystemResult<std::path::PathBuf> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ConfigMsg::GetGlobalConfigPath { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?
    }

    pub async fn workspace_config_path(&self) -> SubsystemResult<std::path::PathBuf> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ConfigMsg::GetWorkspaceConfigPath { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?
    }

    pub async fn tool_config(&self, name: impl Into<String>) -> SubsystemResult<Value> {
        let name = name.into();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ConfigMsg::GetToolConfig {
                name,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(CONFIG_ACTOR))?
    }
}

fn default_global_config_path() -> SubsystemResult<std::path::PathBuf> {
    Ok(BaseDirs::new()
        .ok_or_else(|| SubsystemError::internal("failed to determine config directory"))?
        .config_dir()
        .join("prismagent")
        .join("config.toml"))
}

fn default_workspace_config_path() -> SubsystemResult<std::path::PathBuf> {
    Ok(std::env::current_dir()?
        .join(".prismagent")
        .join("config.toml"))
}

fn read_toml_file<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> SubsystemResult<T> {
    if !path.is_file() {
        return Err(SubsystemError::not_found(
            "config_file",
            path.display().to_string(),
        ));
    }
    let data = std::fs::read_to_string(path)
        .map_err(|error| SubsystemError::io(format!("{}: {error}", path.display())))?;
    toml::from_str(&data)
        .map_err(|error| SubsystemError::invalid_input(format!("{}: {error}", path.display())))
}
