use crate::bus::{
    Bus, Method, ReplyChannel, Request, Response, StreamChunk, Subsystem, SubsystemName,
};
use crate::subsystems::config_subsystem::model::{ConfigSubsystem, FinalModelConfig};
use anyhow::{Context, Result, anyhow};
use directories::BaseDirs;
use serde_json::{Value, json};
use tokio::sync::mpsc;

impl ConfigSubsystem {
    pub fn load() -> Result<Self> {
        let global_config_path = BaseDirs::new()
            .ok_or_else(|| anyhow!("Failed to determine config directory"))?
            .config_dir()
            .join("prismagent")
            .join("config.toml");
        let config_data = std::fs::read_to_string(&global_config_path).with_context(|| {
            format!(
                "Failed to read global config as string: {}",
                global_config_path.display()
            )
        })?;
        let global_config =
            toml::from_str(&config_data).context("Failed to parse global config TOML")?;

        let workspace_config_path = std::env::current_dir()?
            .join(".prismagent")
            .join("config.toml");
        let config_data = std::fs::read_to_string(&workspace_config_path).with_context(|| {
            format!(
                "Failed to read workspace config as string: {}",
                workspace_config_path.display()
            )
        })?;
        let workspace_config =
            toml::from_str(&config_data).context("Failed to parse workspace config TOML")?;

        Ok(Self {
            global_config,
            global_config_path,
            workspace_config,
            workspace_config_path,
        })
    }

    pub fn current_model_config(&self) -> Result<FinalModelConfig> {
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

        Ok(FinalModelConfig {
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
            (Method::Get, "global_config_path") => {
                Response::ok(json!({ "path": self.global_config_path.display().to_string() }))
            }
            (Method::Get, "workspace_config_path") => Response::ok(json!({
                "path": self.workspace_config_path.display().to_string()
            })),
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
        SubsystemName::Config
    }

    fn start(self, _bus: Bus) -> mpsc::Sender<Request> {
        let (tx, mut rx) = mpsc::channel::<Request>(64);
        let mut subsystem = self;

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
