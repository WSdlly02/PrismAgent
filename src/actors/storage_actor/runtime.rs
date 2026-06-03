use crate::actors::storage_actor::model::agent::{Agent, AgentReplaceEntry};
use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::misc::{MiscReadEntry, MiscReplaceEntry, MiscWriteEntry};
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowReplaceEntry};
use crate::actors::storage_actor::model::{STORAGE_ACTOR, StorageActor, StorageHandle, StorageMsg};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

impl StorageActor {
    pub fn load(rx: mpsc::Receiver<StorageMsg>, handles: AppHandles) -> SubsystemResult<Self> {
        let root = directories::BaseDirs::new()
            .ok_or_else(|| SubsystemError::internal("failed to determine home directory"))?
            .home_dir()
            .join(".prismagent")
            .join("workspaces");
        Self::from_root(rx, handles, root)
    }

    pub fn from_root(
        rx: mpsc::Receiver<StorageMsg>,
        handles: AppHandles,
        root: PathBuf,
    ) -> SubsystemResult<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self { rx, root, handles })
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
                StorageMsg::ListAgents {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(self.list_agents(&workspace_uuid));
                }
                StorageMsg::ReadAgents {
                    workspace_uuid,
                    uuids,
                    reply,
                } => {
                    let _ = reply.send(self.read_agents(&workspace_uuid, uuids));
                }
                StorageMsg::WriteAgents {
                    workspace_uuid,
                    agents,
                    reply,
                } => {
                    let _ = reply.send(self.write_agents(&workspace_uuid, &agents));
                }
                StorageMsg::ReplaceAgents {
                    workspace_uuid,
                    entries,
                    reply,
                } => {
                    let _ = reply.send(self.replace_agents(&workspace_uuid, entries));
                }
                StorageMsg::ListUnits {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(self.list_units(&workspace_uuid));
                }
                StorageMsg::ReadUnits {
                    workspace_uuid,
                    uuids,
                    reply,
                } => {
                    let _ = reply.send(self.read_units(&workspace_uuid, uuids));
                }
                StorageMsg::WriteUnits {
                    workspace_uuid,
                    units,
                    reply,
                } => {
                    let _ = reply.send(self.write_units(&workspace_uuid, &units));
                }
                StorageMsg::ListContexts {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(self.list_contexts(&workspace_uuid));
                }
                StorageMsg::ReadContexts {
                    workspace_uuid,
                    uuids,
                    reply,
                } => {
                    let _ = reply.send(self.read_contexts(&workspace_uuid, uuids));
                }
                StorageMsg::WriteContexts {
                    workspace_uuid,
                    contexts,
                    reply,
                } => {
                    let _ = reply.send(self.write_contexts(&workspace_uuid, &contexts));
                }
                StorageMsg::ListWorkflows {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(self.list_workflows(&workspace_uuid));
                }
                StorageMsg::ReadWorkflows {
                    workspace_uuid,
                    uuids,
                    reply,
                } => {
                    let _ = reply.send(self.read_workflows(&workspace_uuid, uuids));
                }
                StorageMsg::WriteWorkflows {
                    workspace_uuid,
                    workflows,
                    reply,
                } => {
                    let _ = reply.send(self.write_workflows(&workspace_uuid, &workflows));
                }
                StorageMsg::ReplaceWorkflows {
                    workspace_uuid,
                    entries,
                    reply,
                } => {
                    let _ = reply.send(self.replace_workflows(&workspace_uuid, entries));
                }
                StorageMsg::ListMisc {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(self.list_misc(&workspace_uuid));
                }
                StorageMsg::ReadMisc {
                    workspace_uuid,
                    names,
                    reply,
                } => {
                    let _ = reply.send(self.read_misc_entries(&workspace_uuid, names));
                }
                StorageMsg::WriteMisc {
                    workspace_uuid,
                    entries,
                    reply,
                } => {
                    let _ = reply.send(self.write_misc_entries(&workspace_uuid, &entries));
                }
                StorageMsg::ReplaceMisc {
                    workspace_uuid,
                    entries,
                    reply,
                } => {
                    let _ = reply.send(self.replace_misc_entries(&workspace_uuid, entries));
                }
            }
        }
    }

    fn list_agents(&self, workspace_uuid: &str) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.workspace_root(workspace_uuid)?.join("agents"))
    }

    fn read_agents(&self, workspace_uuid: &str, uuids: Vec<String>) -> SubsystemResult<Vec<Agent>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.agent_path(workspace_uuid, uuid)?))
            .collect()
    }

    fn write_agents(&self, workspace_uuid: &str, agents: &[Agent]) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(agents.len());
        for agent in agents {
            write_json_create_only(&self.agent_path(workspace_uuid, &agent.uuid)?, agent)?;
            written.push(agent.uuid.clone());
        }
        Ok(written)
    }

    fn replace_agents(
        &self,
        workspace_uuid: &str,
        entries: Vec<AgentReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let mut replaced = Vec::with_capacity(entries.len());
        for entry in entries {
            let new_data = to_pretty_json_vec(&entry.agent)?;
            atomic_replace_file(
                &self.agent_path(workspace_uuid, &entry.uuid)?,
                &entry.old_data,
                &new_data,
            )?;
            replaced.push(entry.uuid);
        }
        Ok(replaced)
    }

    fn list_units(&self, workspace_uuid: &str) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.workspace_root(workspace_uuid)?.join("units"))
    }

    fn read_units(&self, workspace_uuid: &str, uuids: Vec<String>) -> SubsystemResult<Vec<Unit>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.unit_path(workspace_uuid, uuid)?))
            .collect()
    }

    fn write_units(&self, workspace_uuid: &str, units: &[Unit]) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(units.len());
        for unit in units {
            write_json_create_only(&self.unit_path(workspace_uuid, &unit.uuid)?, unit)?;
            written.push(unit.uuid.clone());
        }
        Ok(written)
    }

    fn list_contexts(&self, workspace_uuid: &str) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.workspace_root(workspace_uuid)?.join("contexts"))
    }

    fn read_contexts(
        &self,
        workspace_uuid: &str,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Context>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.context_path(workspace_uuid, uuid)?))
            .collect()
    }

    fn write_contexts(
        &self,
        workspace_uuid: &str,
        contexts: &[Context],
    ) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(contexts.len());
        for context in contexts {
            write_json_create_only(&self.context_path(workspace_uuid, &context.uuid)?, context)?;
            written.push(context.uuid.clone());
        }
        Ok(written)
    }

    fn list_workflows(&self, workspace_uuid: &str) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.workspace_root(workspace_uuid)?.join("workflows"))
    }

    fn read_workflows(
        &self,
        workspace_uuid: &str,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Workflow>> {
        uuids
            .iter()
            .map(|uuid| read_json(&self.workflow_path(workspace_uuid, uuid)?))
            .collect()
    }

    fn write_workflows(
        &self,
        workspace_uuid: &str,
        workflows: &[Workflow],
    ) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(workflows.len());
        for workflow in workflows {
            write_json_create_only(
                &self.workflow_path(workspace_uuid, &workflow.uuid)?,
                workflow,
            )?;
            written.push(workflow.uuid.clone());
        }
        Ok(written)
    }

    fn replace_workflows(
        &self,
        workspace_uuid: &str,
        entries: Vec<WorkflowReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let mut replaced = Vec::with_capacity(entries.len());
        for entry in entries {
            let new_data = to_pretty_json_vec(&entry.workflow)?;
            atomic_replace_file(
                &self.workflow_path(workspace_uuid, &entry.uuid)?,
                &entry.old_data,
                &new_data,
            )?;
            replaced.push(entry.uuid);
        }
        Ok(replaced)
    }

    fn list_misc(&self, workspace_uuid: &str) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.workspace_root(workspace_uuid)?.join("misc"))
    }

    fn read_misc_entries(
        &self,
        workspace_uuid: &str,
        names: Vec<String>,
    ) -> SubsystemResult<Vec<MiscReadEntry>> {
        let mut entries = Vec::with_capacity(names.len());
        for name in names {
            entries.push(MiscReadEntry {
                misc: read_json(&self.misc_path(workspace_uuid, &name)?)?,
                name,
            });
        }
        Ok(entries)
    }

    fn write_misc_entries(
        &self,
        workspace_uuid: &str,
        entries: &[MiscWriteEntry],
    ) -> SubsystemResult<Vec<String>> {
        let mut written = Vec::with_capacity(entries.len());
        for entry in entries {
            write_json_create_only(&self.misc_path(workspace_uuid, &entry.name)?, &entry.misc)?;
            written.push(entry.name.clone());
        }
        Ok(written)
    }

    fn replace_misc_entries(
        &self,
        workspace_uuid: &str,
        entries: Vec<MiscReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let mut replaced = Vec::with_capacity(entries.len());
        for entry in entries {
            let new_data = to_pretty_json_vec(&entry.misc)?;
            atomic_replace_file(
                &self.misc_path(workspace_uuid, &entry.name)?,
                &entry.old_data,
                &new_data,
            )?;
            replaced.push(entry.name);
        }
        Ok(replaced)
    }

    fn workspace_root(&self, workspace_uuid: &str) -> SubsystemResult<PathBuf> {
        if !is_safe_object_name(workspace_uuid) {
            return Err(SubsystemError::invalid_input(format!(
                "invalid workspace uuid: {workspace_uuid}"
            )));
        }
        let root = self.root.join(workspace_uuid);
        if !root.is_dir() {
            return Err(SubsystemError::not_found("workspace", workspace_uuid));
        }
        Ok(root)
    }

    fn agent_path(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<PathBuf> {
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("agents")
            .join(format!("{uuid}.json")))
    }

    fn unit_path(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<PathBuf> {
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("units")
            .join(format!("{uuid}.json")))
    }

    fn context_path(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<PathBuf> {
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("contexts")
            .join(format!("{uuid}.json")))
    }

    fn workflow_path(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<PathBuf> {
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("workflows")
            .join(format!("{uuid}.json")))
    }

    fn misc_path(&self, workspace_uuid: &str, name: &str) -> SubsystemResult<PathBuf> {
        if !is_safe_object_name(name) {
            return Err(SubsystemError::invalid_input(format!(
                "invalid misc name: {name}"
            )));
        }
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("misc")
            .join(format!("{name}.json")))
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

    pub async fn list_agents(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListAgents {
                workspace_uuid: workspace_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_agents(
        &self,
        workspace_uuid: impl Into<String>,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Agent>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadAgents {
                workspace_uuid: workspace_uuid.into(),
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_agents(
        &self,
        workspace_uuid: impl Into<String>,
        agents: Vec<Agent>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteAgents {
                workspace_uuid: workspace_uuid.into(),
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
        workspace_uuid: impl Into<String>,
        entries: Vec<AgentReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReplaceAgents {
                workspace_uuid: workspace_uuid.into(),
                entries,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_units(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListUnits {
                workspace_uuid: workspace_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_units(
        &self,
        workspace_uuid: impl Into<String>,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Unit>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadUnits {
                workspace_uuid: workspace_uuid.into(),
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_units(
        &self,
        workspace_uuid: impl Into<String>,
        units: Vec<Unit>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteUnits {
                workspace_uuid: workspace_uuid.into(),
                units,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_contexts(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListContexts {
                workspace_uuid: workspace_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_contexts(
        &self,
        workspace_uuid: impl Into<String>,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Context>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadContexts {
                workspace_uuid: workspace_uuid.into(),
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_contexts(
        &self,
        workspace_uuid: impl Into<String>,
        contexts: Vec<Context>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteContexts {
                workspace_uuid: workspace_uuid.into(),
                contexts,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_workflows(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListWorkflows {
                workspace_uuid: workspace_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_workflows(
        &self,
        workspace_uuid: impl Into<String>,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Workflow>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadWorkflows {
                workspace_uuid: workspace_uuid.into(),
                uuids,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_workflows(
        &self,
        workspace_uuid: impl Into<String>,
        workflows: Vec<Workflow>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteWorkflows {
                workspace_uuid: workspace_uuid.into(),
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
        workspace_uuid: impl Into<String>,
        entries: Vec<WorkflowReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReplaceWorkflows {
                workspace_uuid: workspace_uuid.into(),
                entries,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn list_misc(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ListMisc {
                workspace_uuid: workspace_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn read_misc(
        &self,
        workspace_uuid: impl Into<String>,
        names: Vec<String>,
    ) -> SubsystemResult<Vec<MiscReadEntry>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReadMisc {
                workspace_uuid: workspace_uuid.into(),
                names,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(STORAGE_ACTOR))?
    }

    pub async fn write_misc(
        &self,
        workspace_uuid: impl Into<String>,
        entries: Vec<MiscWriteEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::WriteMisc {
                workspace_uuid: workspace_uuid.into(),
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
        workspace_uuid: impl Into<String>,
        entries: Vec<MiscReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(StorageMsg::ReplaceMisc {
                workspace_uuid: workspace_uuid.into(),
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
