use crate::actors::storage_actor::model::agent::{Agent, AgentReplaceEntry};
use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::misc::{MiscReadEntry, MiscReplaceEntry, MiscWriteEntry};
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowReplaceEntry};
use crate::actors::storage_actor::model::{STORAGE_ACTOR, StorageActor, StorageHandle, StorageMsg};
use crate::error::{SubsystemError, SubsystemResult};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

impl StorageActor {
    pub fn load(rx: mpsc::Receiver<StorageMsg>) -> SubsystemResult<Self> {
        let root = std::env::current_dir()?.join(".prismagent");
        Self::from_root(rx, root)
    }

    pub fn from_root(rx: mpsc::Receiver<StorageMsg>, root: PathBuf) -> SubsystemResult<Self> {
        std::fs::create_dir_all(root.join("agents"))?;
        std::fs::create_dir_all(root.join("units"))?;
        std::fs::create_dir_all(root.join("contexts"))?;
        std::fs::create_dir_all(root.join("workflows"))?;
        std::fs::create_dir_all(root.join("misc"))?;
        Ok(Self { rx, root })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                StorageMsg::Root { reply } => {
                    let _ = reply.send(Ok(self.root.clone()));
                }
                StorageMsg::ListAgents { reply } => {
                    let _ = reply.send(self.list_agents());
                }
                StorageMsg::ReadAgents { uuids, reply } => {
                    let _ = reply.send(self.read_agents(uuids));
                }
                StorageMsg::WriteAgents { agents, reply } => {
                    let _ = reply.send(self.write_agents(&agents));
                }
                StorageMsg::ReplaceAgents { entries, reply } => {
                    let _ = reply.send(self.replace_agents(entries));
                }
                StorageMsg::ListUnits { reply } => {
                    let _ = reply.send(self.list_units());
                }
                StorageMsg::ReadUnits { uuids, reply } => {
                    let _ = reply.send(self.read_units(uuids));
                }
                StorageMsg::WriteUnits { units, reply } => {
                    let _ = reply.send(self.write_units(&units));
                }
                StorageMsg::ListContexts { reply } => {
                    let _ = reply.send(self.list_contexts());
                }
                StorageMsg::ReadContexts { uuids, reply } => {
                    let _ = reply.send(self.read_contexts(uuids));
                }
                StorageMsg::WriteContexts { contexts, reply } => {
                    let _ = reply.send(self.write_contexts(&contexts));
                }
                StorageMsg::ListWorkflows { reply } => {
                    let _ = reply.send(self.list_workflows());
                }
                StorageMsg::ReadWorkflows { uuids, reply } => {
                    let _ = reply.send(self.read_workflows(uuids));
                }
                StorageMsg::WriteWorkflows { workflows, reply } => {
                    let _ = reply.send(self.write_workflows(&workflows));
                }
                StorageMsg::ReplaceWorkflows { entries, reply } => {
                    let _ = reply.send(self.replace_workflows(entries));
                }
                StorageMsg::ListMisc { reply } => {
                    let _ = reply.send(self.list_misc());
                }
                StorageMsg::ReadMisc { names, reply } => {
                    let _ = reply.send(self.read_misc_entries(names));
                }
                StorageMsg::WriteMisc { entries, reply } => {
                    let _ = reply.send(self.write_misc_entries(&entries));
                }
                StorageMsg::ReplaceMisc { entries, reply } => {
                    let _ = reply.send(self.replace_misc_entries(entries));
                }
            }
        }
    }

    fn list_agents(&self) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.root.join("agents"))
    }

    fn read_agents(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Agent>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.agent_path(uuid)))
            .collect()
    }

    fn write_agents(&self, agents: &[Agent]) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(agents.len());
        for agent in agents {
            write_json_create_only(&self.agent_path(&agent.uuid), agent)?;
            written.push(agent.uuid.clone());
        }
        Ok(written)
    }

    fn replace_agents(&self, entries: Vec<AgentReplaceEntry>) -> SubsystemResult<Vec<String>> {
        let mut replaced = Vec::with_capacity(entries.len());
        for entry in entries {
            let new_data = to_pretty_json_vec(&entry.agent)?;
            atomic_replace_file(&self.agent_path(&entry.uuid), &entry.old_data, &new_data)?;
            replaced.push(entry.uuid);
        }
        Ok(replaced)
    }

    fn list_units(&self) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.root.join("units"))
    }

    fn read_units(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Unit>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.unit_path(uuid)))
            .collect()
    }

    fn write_units(&self, units: &[Unit]) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(units.len());
        for unit in units {
            write_json_create_only(&self.unit_path(&unit.uuid), unit)?;
            written.push(unit.uuid.clone());
        }
        Ok(written)
    }

    fn list_contexts(&self) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.root.join("contexts"))
    }

    fn read_contexts(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Context>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.context_path(uuid)))
            .collect()
    }

    fn write_contexts(&self, contexts: &[Context]) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(contexts.len());
        for context in contexts {
            write_json_create_only(&self.context_path(&context.uuid), context)?;
            written.push(context.uuid.clone());
        }
        Ok(written)
    }

    fn list_workflows(&self) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.root.join("workflows"))
    }

    fn read_workflows(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Workflow>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.workflow_path(uuid)))
            .collect()
    }

    fn write_workflows(&self, workflows: &[Workflow]) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(workflows.len());
        for workflow in workflows {
            write_json_create_only(&self.workflow_path(&workflow.uuid), workflow)?;
            written.push(workflow.uuid.clone());
        }
        Ok(written)
    }

    fn replace_workflows(
        &self,
        entries: Vec<WorkflowReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let mut replaced = Vec::with_capacity(entries.len());
        for entry in entries {
            let new_data = to_pretty_json_vec(&entry.workflow)?;
            atomic_replace_file(&self.workflow_path(&entry.uuid), &entry.old_data, &new_data)?;
            replaced.push(entry.uuid);
        }
        Ok(replaced)
    }

    fn list_misc(&self) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.root.join("misc"))
    }

    fn read_misc_entries(&self, names: Vec<String>) -> SubsystemResult<Vec<MiscReadEntry>> {
        let mut entries = Vec::with_capacity(names.len());
        for name in names {
            entries.push(MiscReadEntry {
                misc: read_json(&self.misc_path(&name)?)?,
                name,
            });
        }
        Ok(entries)
    }

    fn write_misc_entries(&self, entries: &[MiscWriteEntry]) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(entries.len());
        for entry in entries {
            write_json_create_only(&self.misc_path(&entry.name)?, &entry.misc)?;
            written.push(entry.name.clone());
        }
        Ok(written)
    }

    fn replace_misc_entries(&self, entries: Vec<MiscReplaceEntry>) -> SubsystemResult<Vec<String>> {
        let mut replaced = Vec::with_capacity(entries.len());
        for entry in entries {
            let new_data = to_pretty_json_vec(&entry.misc)?;
            atomic_replace_file(&self.misc_path(&entry.name)?, &entry.old_data, &new_data)?;
            replaced.push(entry.name);
        }
        Ok(replaced)
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

    fn misc_path(&self, name: &str) -> SubsystemResult<PathBuf> {
        if !is_safe_object_name(name) {
            return Err(SubsystemError::invalid_input(format!(
                "invalid misc name: {name}"
            )));
        }
        Ok(self.root.join("misc").join(format!("{name}.json")))
    }
}

impl StorageHandle {
    pub async fn root(&self) -> SubsystemResult<PathBuf> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::Root { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_agents(&self) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListAgents { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_agents(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Agent>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadAgents {
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_agents(&self, agents: Vec<Agent>) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteAgents {
                agents,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn replace_agents(
        &self,
        entries: Vec<AgentReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReplaceAgents {
                entries,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_units(&self) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListUnits { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_units(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Unit>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadUnits {
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_units(&self, units: Vec<Unit>) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteUnits {
                units,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_contexts(&self) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListContexts { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_contexts(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Context>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadContexts {
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_contexts(&self, contexts: Vec<Context>) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteContexts {
                contexts,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_workflows(&self) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListWorkflows { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_workflows(&self, uuids: Vec<String>) -> SubsystemResult<Vec<Workflow>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadWorkflows {
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_workflows(&self, workflows: Vec<Workflow>) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteWorkflows {
                workflows,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn replace_workflows(
        &self,
        entries: Vec<WorkflowReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReplaceWorkflows {
                entries,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_misc(&self) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListMisc { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_misc(&self, names: Vec<String>) -> SubsystemResult<Vec<MiscReadEntry>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadMisc {
                names,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_misc(&self, entries: Vec<MiscWriteEntry>) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteMisc {
                entries,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn replace_misc(
        &self,
        entries: Vec<MiscReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReplaceMisc {
                entries,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }
}

fn atomic_create_file(dst: &Path, data: &[u8]) -> SubsystemResult<()> {
    if dst.exists() {
        return Err(SubsystemError::Conflict {
            resource: "file",
            id: dst.display().to_string(),
        });
    }
    std::fs::create_dir_all(
        dst.parent()
            .ok_or_else(|| SubsystemError::internal("invalid path: no parent directory"))?,
    )?;
    let tmp_dst = dst.with_extension("tmp");
    std::fs::write(&tmp_dst, data)?;
    std::fs::rename(tmp_dst, dst)?;
    Ok(())
}

fn atomic_replace_file(dst: &Path, old: &[u8], new: &[u8]) -> SubsystemResult<()> {
    if !dst.exists() {
        return Err(SubsystemError::not_found("file", dst.display().to_string()));
    }
    let current_data = std::fs::read(dst)?;
    if current_data != old {
        return Err(SubsystemError::Conflict {
            resource: "file",
            id: dst.display().to_string(),
        });
    }
    let tmp_dst = dst.with_extension("tmp");
    std::fs::write(&tmp_dst, new)?;
    std::fs::rename(tmp_dst, dst)?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> SubsystemResult<T> {
    if !path.is_file() {
        return Err(SubsystemError::not_found(
            "file",
            path.display().to_string(),
        ));
    }
    let data = std::fs::read(path)?;
    serde_json::from_slice(&data)
        .map_err(|error| SubsystemError::invalid_input(format!("{}: {error}", path.display())))
}

fn write_json_create_only<T: serde::Serialize>(path: &Path, value: &T) -> SubsystemResult<()> {
    let data = to_pretty_json_vec(value)?;
    atomic_create_file(path, &data)
}

fn to_pretty_json_vec<T: serde::Serialize>(value: &T) -> SubsystemResult<Vec<u8>> {
    let mut data = Vec::new();
    serde_json::to_writer_pretty(&mut data, value)
        .map_err(|error| SubsystemError::invalid_input(error.to_string()))?;
    data.push(b'\n');
    Ok(data)
}

fn list_json_object_ids(dir: &Path) -> SubsystemResult<Vec<String>> {
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
