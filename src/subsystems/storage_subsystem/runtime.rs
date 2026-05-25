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
use crate::subsystems::storage_subsystem::model::misc::{
    Misc, MiscReadRequest, MiscReplaceRequest, MiscWriteRequest,
};
use crate::subsystems::storage_subsystem::model::unit::{Unit, UnitReadRequest, UnitWriteRequest};
use crate::subsystems::storage_subsystem::model::workflow::{
    Workflow, WorkflowReadRequest, WorkflowReplaceRequest, WorkflowWriteRequest,
};
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
        std::fs::create_dir_all(root.join("workflows"))?;
        std::fs::create_dir_all(root.join("misc"))?;
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

    pub fn read_workflow(&self, uuid: &str) -> Result<Workflow> {
        read_json(&self.workflow_path(uuid))
    }

    pub fn list_workflows(&self) -> Result<Vec<String>> {
        list_json_object_ids(&self.root.join("workflows"))
    }

    pub fn write_workflow(&self, workflow: &Workflow) -> Result<()> {
        write_json_create_only(&self.workflow_path(&workflow.uuid), workflow)
    }

    pub fn replace_workflow(&self, uuid: &str, old_data: &[u8], workflow: &Workflow) -> Result<()> {
        let new_data = to_pretty_json_vec(workflow)?;
        atomic_replace_file(&self.workflow_path(uuid), old_data, &new_data)
    }

    pub fn read_misc(&self, name: &str) -> Result<Misc> {
        read_json(&self.misc_path(name)?)
    }

    pub fn list_misc(&self) -> Result<Vec<String>> {
        list_json_object_ids(&self.root.join("misc"))
    }

    pub fn write_misc(&self, name: &str, misc: &Misc) -> Result<()> {
        write_json_create_only(&self.misc_path(name)?, misc)
    }

    pub fn replace_misc(&self, name: &str, old_data: &[u8], misc: &Misc) -> Result<()> {
        let new_data = to_pretty_json_vec(misc)?;
        atomic_replace_file(&self.misc_path(name)?, old_data, &new_data)
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

    fn workflow_path(&self, uuid: &str) -> PathBuf {
        self.root.join("workflows").join(format!("{uuid}.json"))
    }

    fn misc_path(&self, name: &str) -> Result<PathBuf> {
        if !is_safe_object_name(name) {
            return Err(anyhow!("Invalid misc name: {name}"));
        }
        Ok(self.root.join("misc").join(format!("{name}.json")))
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
                let mut agents = Vec::new();
                let mut failed = Vec::new();
                for uuid in request.uuids {
                    match self.read_agent(&uuid) {
                        Ok(agent) => agents.push(agent),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "agents": agents, "failed": failed }))
            }
            (Method::Post, "agent/write") => {
                let request = match response_body_as::<AgentWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut written = Vec::new();
                let mut failed = Vec::new();
                for agent in request.agents {
                    let uuid = agent.uuid.clone();
                    match self.write_agent(&agent) {
                        Ok(()) => written.push(uuid),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "written": written, "failed": failed }))
            }
            (Method::Post, "agent/replace") => {
                let request = match response_body_as::<AgentReplaceRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut replaced = Vec::new();
                let mut failed = Vec::new();
                for entry in request.entries {
                    match self.replace_agent(&entry.uuid, &entry.old_data, &entry.agent) {
                        Ok(()) => replaced.push(entry.uuid),
                        Err(error) => failed.push(json!({
                            "uuid": entry.uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "replaced": replaced, "failed": failed }))
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
                let mut units = Vec::new();
                let mut failed = Vec::new();
                for uuid in request.uuids {
                    match self.read_unit(&uuid) {
                        Ok(unit) => units.push(unit),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "units": units, "failed": failed }))
            }
            (Method::Post, "unit/write") => {
                let request = match response_body_as::<UnitWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut written = Vec::new();
                let mut failed = Vec::new();
                for unit in request.units {
                    let uuid = unit.uuid.clone();
                    match self.write_unit(&unit) {
                        Ok(()) => written.push(uuid),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "written": written, "failed": failed }))
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
                let mut contexts = Vec::new();
                let mut failed = Vec::new();
                for uuid in request.uuids {
                    match self.read_context(&uuid) {
                        Ok(context) => contexts.push(context),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "contexts": contexts, "failed": failed }))
            }
            (Method::Post, "context/write") => {
                let request = match response_body_as::<ContextWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut written = Vec::new();
                let mut failed = Vec::new();
                for context in request.contexts {
                    let uuid = context.uuid.clone();
                    match self.write_context(&context) {
                        Ok(()) => written.push(uuid),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "written": written, "failed": failed }))
            }
            (Method::Get, "workflow/list") => match self.list_workflows() {
                Ok(workflows) => Response::ok(json!({ "workflows": workflows })),
                Err(error) => Response::internal_error(error),
            },
            (Method::Post, "workflow/read") => {
                let request = match response_body_as::<WorkflowReadRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut workflows = Vec::new();
                let mut failed = Vec::new();
                for uuid in request.uuids {
                    match self.read_workflow(&uuid) {
                        Ok(workflow) => workflows.push(workflow),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "workflows": workflows, "failed": failed }))
            }
            (Method::Post, "workflow/write") => {
                let request = match response_body_as::<WorkflowWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut written = Vec::new();
                let mut failed = Vec::new();
                for workflow in request.workflows {
                    let uuid = workflow.uuid.clone();
                    match self.write_workflow(&workflow) {
                        Ok(()) => written.push(uuid),
                        Err(error) => failed.push(json!({
                            "uuid": uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "written": written, "failed": failed }))
            }
            (Method::Post, "workflow/replace") => {
                let request = match response_body_as::<WorkflowReplaceRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut replaced = Vec::new();
                let mut failed = Vec::new();
                for entry in request.entries {
                    match self.replace_workflow(&entry.uuid, &entry.old_data, &entry.workflow) {
                        Ok(()) => replaced.push(entry.uuid),
                        Err(error) => failed.push(json!({
                            "uuid": entry.uuid,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "replaced": replaced, "failed": failed }))
            }
            (Method::Get, "misc/list") => match self.list_misc() {
                Ok(misc) => Response::ok(json!({ "misc": misc })),
                Err(error) => Response::internal_error(error),
            },
            (Method::Post, "misc/read") => {
                let request = match response_body_as::<MiscReadRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut misc = Vec::new();
                let mut failed = Vec::new();
                for name in request.names {
                    match self.read_misc(&name) {
                        Ok(value) => misc.push(json!({
                            "name": name,
                            "misc": value,
                        })),
                        Err(error) => failed.push(json!({
                            "name": name,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "misc": misc, "failed": failed }))
            }
            (Method::Post, "misc/write") => {
                let request = match response_body_as::<MiscWriteRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut written = Vec::new();
                let mut failed = Vec::new();
                for entry in request.entries {
                    match self.write_misc(&entry.name, &entry.misc) {
                        Ok(()) => written.push(entry.name),
                        Err(error) => failed.push(json!({
                            "name": entry.name,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "written": written, "failed": failed }))
            }
            (Method::Post, "misc/replace") => {
                let request = match response_body_as::<MiscReplaceRequest>(req.body.clone()) {
                    Ok(request) => request,
                    Err(error) => return Response::bad_request(error),
                };
                let mut replaced = Vec::new();
                let mut failed = Vec::new();
                for entry in request.entries {
                    match self.replace_misc(&entry.name, &entry.old_data, &entry.misc) {
                        Ok(()) => replaced.push(entry.name),
                        Err(error) => failed.push(json!({
                            "name": entry.name,
                            "error": error.to_string(),
                        })),
                    }
                }
                Response::ok(json!({ "replaced": replaced, "failed": failed }))
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

fn is_safe_object_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
        && !name.ends_with(".json")
}
