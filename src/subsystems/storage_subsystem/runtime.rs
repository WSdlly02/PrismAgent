use crate::bus::{
    Bus, Method, ReplyChannel, Request, Response, StreamChunk, Subsystem, SubsystemName,
};
use crate::subsystems::response_body_as;
use crate::subsystems::storage_subsystem::model::StorageSubsystem;
use crate::subsystems::storage_subsystem::model::agent::{
    Agent, AgentReadRequest, AgentReplaceRequest, AgentWriteRequest,
};
use crate::subsystems::storage_subsystem::model::context::{
    Context, ContextReadRequest, ContextWriteRequest,
};
use crate::subsystems::storage_subsystem::model::unit::{Unit, UnitReadRequest, UnitWriteRequest};
use anyhow::{Result, anyhow};
use serde_json::json;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

impl StorageSubsystem {
    pub fn load() -> Result<Self> {
        let root = std::env::current_dir()?.join(".prismagent");
        Self::from_root(root)
    }

    pub fn from_root(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("agents"))?;
        std::fs::create_dir_all(root.join("units"))?;
        std::fs::create_dir_all(root.join("contexts"))?;
        Ok(Self { root })
    }

    pub fn read_agent(&self, uuid: &str) -> Result<Agent> {
        read_json(&self.agent_path(uuid))
    }

    pub fn list_agents(&self) -> Result<Vec<String>> {
        list_json_object_ids(&self.root.join("agents"))
    }

    pub fn write_agent(&self, agent: &Agent) -> Result<()> {
        write_json_create_only(&self.agent_path(&agent.uuid), agent)
    }

    pub fn replace_agent(&self, uuid: &str, old_data: &[u8], agent: &Agent) -> Result<()> {
        let new_data = to_pretty_json_vec(agent)?;
        atomic_replace_file(&self.agent_path(uuid), old_data, &new_data)
    }

    pub fn read_unit(&self, uuid: &str) -> Result<Unit> {
        read_json(&self.unit_path(uuid))
    }

    pub fn list_units(&self) -> Result<Vec<String>> {
        list_json_object_ids(&self.root.join("units"))
    }

    pub fn write_unit(&self, unit: &Unit) -> Result<()> {
        write_json_create_only(&self.unit_path(&unit.uuid), unit)
    }

    pub fn read_context(&self, uuid: &str) -> Result<Context> {
        read_json(&self.context_path(uuid))
    }

    pub fn list_contexts(&self) -> Result<Vec<String>> {
        list_json_object_ids(&self.root.join("contexts"))
    }

    pub fn write_context(&self, context: &Context) -> Result<()> {
        write_json_create_only(&self.context_path(&context.uuid), context)
    }

    fn agent_path(&self, uuid: &str) -> PathBuf {
        self.root.join("agents").join(format!("{uuid}.json"))
    }

    fn unit_path(&self, uuid: &str) -> PathBuf {
        self.root.join("units").join(format!("{uuid}.json"))
    }

    fn context_path(&self, uuid: &str) -> PathBuf {
        self.root.join("contexts").join(format!("{uuid}.json"))
    }

    fn handle_request(&self, req: &Request) -> Response {
        match (req.method, req.path.as_str()) {
            (Method::Get, "root") => {
                Response::ok(json!({ "root": self.root.display().to_string() }))
            }
            (Method::Get, "agent/list") => match self.list_agents() {
                Ok(agents) => Response::ok(json!({ "agents": agents })),
                Err(error) => Response::internal_error(error),
            },
            (Method::Post, "agent/read") => {
                let request = match response_body_as::<AgentReadRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.read_agent(&request.uuid) {
                    Ok(agent) => Response::ok(json!(agent)),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "agent/write") => {
                let request = match response_body_as::<AgentWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.write_agent(&request.agent) {
                    Ok(()) => Response::ok(json!({ "status": "ok" })),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "agent/replace") => {
                let request = match response_body_as::<AgentReplaceRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.replace_agent(&request.uuid, &request.old_data, &request.agent) {
                    Ok(()) => Response::ok(json!({ "status": "ok" })),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Get, "unit/list") => match self.list_units() {
                Ok(units) => Response::ok(json!({ "units": units })),
                Err(error) => Response::internal_error(error),
            },
            (Method::Post, "unit/read") => {
                let request = match response_body_as::<UnitReadRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.read_unit(&request.uuid) {
                    Ok(unit) => Response::ok(json!(unit)),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "unit/write") => {
                let request = match response_body_as::<UnitWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.write_unit(&request.unit) {
                    Ok(()) => Response::ok(json!({ "status": "ok" })),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Get, "context/list") => match self.list_contexts() {
                Ok(contexts) => Response::ok(json!({ "contexts": contexts })),
                Err(error) => Response::internal_error(error),
            },
            (Method::Post, "context/read") => {
                let request = match response_body_as::<ContextReadRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.read_context(&request.uuid) {
                    Ok(context) => Response::ok(json!(context)),
                    Err(error) => Response::internal_error(error),
                }
            }
            (Method::Post, "context/write") => {
                let request = match response_body_as::<ContextWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                match self.write_context(&request.context) {
                    Ok(()) => Response::ok(json!({ "status": "ok" })),
                    Err(error) => Response::internal_error(error),
                }
            }
            _ => Response::not_found(req.path.as_str()),
        }
    }
}

impl Subsystem for StorageSubsystem {
    fn name(&self) -> SubsystemName {
        SubsystemName::Storage
    }

    fn start(self, _bus: Bus) -> mpsc::Sender<Request> {
        let (tx, mut rx) = mpsc::channel::<Request>(64);
        let subsystem = self;

        tokio::spawn(async move {
            while let Some(req) = rx.recv().await {
                let response = subsystem.handle_request(&req);
                match req.reply {
                    ReplyChannel::Once(tx) => {
                        let _ = tx.send(response);
                    }
                    ReplyChannel::Stream(tx) => {
                        let _ = tx.send(StreamChunk::Delta(response.body)).await;
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

pub(crate) fn atomic_create_file(dst: &Path, data: &[u8]) -> Result<()> {
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

pub(crate) fn atomic_replace_file(dst: &Path, old: &[u8], new: &[u8]) -> Result<()> {
    if !dst.exists() {
        return Err(anyhow!("File does not exist: {}", dst.display()));
    }
    let current_data = std::fs::read(dst)?;
    if current_data != old {
        return Err(anyhow!("File content does not match expected old content"));
    }
    let tmp_dst = dst.with_extension("tmp");
    std::fs::write(&tmp_dst, new)?;
    std::fs::rename(tmp_dst, dst)?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let data = std::fs::read(path).map_err(|e| anyhow!("Failed to read {:?}: {}", path, e))?;
    serde_json::from_slice(&data).map_err(|e| anyhow!("Failed to parse JSON {:?}: {}", path, e))
}

fn write_json_create_only<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let data = to_pretty_json_vec(value)?;
    atomic_create_file(path, &data)
}

fn to_pretty_json_vec<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    serde_json::to_writer_pretty(&mut data, value)
        .map_err(|e| anyhow!("Failed to serialize pretty JSON: {e}"))?;
    data.push(b'\n');
    Ok(data)
}

fn list_json_object_ids(dir: &Path) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    if !dir.exists() {
        return Ok(ids);
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        ids.push(stem.to_string());
    }
    ids.sort();
    Ok(ids)
}
