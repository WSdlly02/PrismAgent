use crate::bus::{
    Bus, Method, ReplyChannel, Request, Response, StreamChunk, Subsystem, SubsystemName,
};
use crate::model::app::{GlobalConfig, ModelConfig};
use anyhow::{Context, Result, anyhow};
use directories::BaseDirs;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

pub const CONFIG_SUBSYSTEM: SubsystemName = SubsystemName::Config;

pub struct ConfigSubsystem {
    config_path: PathBuf,
    global_config: GlobalConfig,
}

impl ConfigSubsystem {
    pub fn from_default_config_file() -> Result<Self> {
        Self::from_config_file(default_config_path()?)
    }

    pub fn from_config_file(config_path: impl Into<PathBuf>) -> Result<Self> {
        let config_path = config_path.into();
        let global_config = read_global_config_from_path(&config_path)?;
        Ok(Self {
            config_path,
            global_config,
        })
    }

    pub fn global_config(&self) -> &GlobalConfig {
        &self.global_config
    }

    pub fn current_model_config(&self) -> Result<ModelConfig> {
        let provider = self
            .global_config
            .providers
            .get(&self.global_config.current_provider)
            .ok_or_else(|| {
                anyhow!(
                    "Current provider '{}' not found in global config providers",
                    self.global_config.current_provider
                )
            })?;

        Ok(ModelConfig {
            final_api_key: provider.api_key.clone(),
            final_model: provider.model.clone(),
        })
    }

    fn handle_request(&mut self, req: &Request) -> Response {
        match (req.method, req.path.as_str()) {
            (Method::Get, "global_config") => Response::ok(json!(self.global_config)),
            (Method::Get, "current_provider") => {
                match self
                    .global_config
                    .providers
                    .get(&self.global_config.current_provider)
                {
                    Some(provider) => Response::ok(json!({
                        "name": self.global_config.current_provider,
                        "provider": provider,
                    })),
                    None => Response::internal_error(format!(
                        "Current provider '{}' not found in global config providers",
                        self.global_config.current_provider
                    )),
                }
            }
            (Method::Get, "model_config") => match self.current_model_config() {
                Ok(config) => Response::ok(json!(config)),
                Err(error) => Response::internal_error(error),
            },
            (Method::Get, "config_path") => {
                Response::ok(json!({ "path": self.config_path.display().to_string() }))
            }
            (Method::Post, "tool_config") => {
                let Some(name) = req.body.get("name").and_then(Value::as_str) else {
                    return Response::bad_request("missing tool config name");
                };
                match self.global_config.tools.get(name) {
                    Some(config) => Response::ok(config.clone()),
                    None => Response::not_found(format!("tool_config/{name}")),
                }
            }
            _ => Response::not_found(req.path.as_str()),
        }
    }
}

impl Subsystem for ConfigSubsystem {
    fn name(&self) -> SubsystemName {
        CONFIG_SUBSYSTEM
    }

    fn start(self: Box<Self>, _bus: Bus) -> mpsc::Sender<Request> {
        let (tx, mut rx) = mpsc::channel::<Request>(64);
        let mut subsystem = *self;

        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let response = subsystem.handle_request(&req);
                match req.reply {
                    ReplyChannel::Once(tx) => {
                        let _ = tx.send(response);
                    }
                    ReplyChannel::Stream(tx) => {
                        let _ = tx
                            .send(StreamChunk::Error(
                                "ConfigSubsystem does not support stream replies.".to_string(),
                            ))
                            .await;
                        let _ = tx.send(StreamChunk::Done).await;
                    }
                    ReplyChannel::None => {
                        let _ = response;
                    }
                }
            }
        });

        tx
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(BaseDirs::new()
        .ok_or_else(|| anyhow!("Failed to determine config directory"))?
        .config_dir()
        .join("prismagent")
        .join("settings.toml"))
}

fn read_global_config_from_path(config_path: &Path) -> Result<GlobalConfig> {
    let config_data = std::fs::read_to_string(config_path).with_context(|| {
        format!(
            "Failed to read global config as string: {}",
            config_path.display()
        )
    })?;
    toml::from_str(&config_data).context("Failed to parse global config TOML")
}

pub fn response_body_as<T: serde::de::DeserializeOwned>(body: Value) -> Result<T> {
    serde_json::from_value(body).context("Failed to deserialize config response body")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{Bus, ResponseStatus};
    use std::fs;
    use uuid::Uuid;

    fn write_test_config() -> PathBuf {
        let root = std::env::temp_dir().join(format!("prismagent-config-{}", Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.toml");
        fs::write(
            &path,
            r#"
current_provider = "deepseek"

[providers.deepseek]
api_key = "test-key"
model = "deepseek-chat"

[tools.tinyfish]
api_key = "tinyfish-key"
"#,
        )
        .unwrap();
        path
    }

    #[test]
    fn reads_global_config_and_resolves_current_model_config() {
        let subsystem = ConfigSubsystem::from_config_file(write_test_config()).unwrap();
        let model_config = subsystem.current_model_config().unwrap();

        assert_eq!(subsystem.global_config().current_provider, "deepseek");
        assert_eq!(
            model_config,
            ModelConfig {
                final_api_key: "test-key".to_string(),
                final_model: "deepseek-chat".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn returns_model_config_through_bus() {
        let bus = Bus::new();
        let subsystem = ConfigSubsystem::from_config_file(write_test_config()).unwrap();
        let name = subsystem.name();
        let tx = Box::new(subsystem).start(bus.clone());
        bus.register(name, tx).await;

        let response = bus
            .get(CONFIG_SUBSYSTEM, SubsystemName::Shell, "model_config")
            .await
            .unwrap();
        assert_eq!(response.status, ResponseStatus::Ok);
        let model_config: ModelConfig = response_body_as(response.body).unwrap();

        assert_eq!(model_config.final_model, "deepseek-chat");
        assert_eq!(model_config.final_api_key, "test-key");
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_route() {
        let bus = Bus::new();
        let subsystem = ConfigSubsystem::from_config_file(write_test_config()).unwrap();
        let name = subsystem.name();
        let tx = Box::new(subsystem).start(bus.clone());
        bus.register(name, tx).await;

        let response = bus
            .get(CONFIG_SUBSYSTEM, SubsystemName::Shell, "missing")
            .await
            .unwrap();

        assert_eq!(response.status, ResponseStatus::NotFound);
    }

    #[tokio::test]
    async fn returns_tool_config_through_bus() {
        let bus = Bus::new();
        let subsystem = ConfigSubsystem::from_config_file(write_test_config()).unwrap();
        let name = subsystem.name();
        let tx = Box::new(subsystem).start(bus.clone());
        bus.register(name, tx).await;

        let response = bus
            .post(
                CONFIG_SUBSYSTEM,
                SubsystemName::Shell,
                "tool_config",
                json!({ "name": "tinyfish" }),
            )
            .await
            .unwrap();

        assert_eq!(response.status, ResponseStatus::Ok);
        assert_eq!(response.body["api_key"], "tinyfish-key");
    }
}
