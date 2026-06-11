use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest, AgentUpdateRequest};
use crate::actors::storage_actor::model::context::{Context, ContextCreateRequest};
use crate::actors::storage_actor::model::misc::{MiscReadEntry, MiscReplaceEntry, MiscWriteEntry};
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowCreateRequest};
use crate::actors::storage_actor::model::{STORAGE_ACTOR, StorageActor, StorageHandle, StorageMsg};
use crate::error::{SubsystemError, SubsystemResult};
use crate::{actor_dispatch, impl_handle_methods};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

impl StorageActor {
    pub fn load(rx: mpsc::Receiver<StorageMsg>) -> SubsystemResult<Self> {
        let root = dirs::home_dir()
            .ok_or_else(|| SubsystemError::internal("failed to determine home directory"))?
            .join(".prismagent")
            .join("workspaces");
        Self::from_root(rx, root)
    }

    pub fn from_root(rx: mpsc::Receiver<StorageMsg>, root: PathBuf) -> SubsystemResult<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self { rx, root })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            actor_dispatch!(msg;
                StorageMsg::Root { ; reply } => Ok(self.root.clone()),
                StorageMsg::ListAgents { workspace_uuid ; reply } => self.list_agents(&workspace_uuid),
                StorageMsg::ReadAgents { workspace_uuid, uuids ; reply } => self.read_agents(&workspace_uuid, uuids),
                StorageMsg::WriteAgents { workspace_uuid, agents ; reply } => self.write_agents(&workspace_uuid, &agents),
                StorageMsg::CreateAgent { request, auto_loop, auto_loop_message ; reply } => self.create_agent(request, auto_loop, auto_loop_message),
                StorageMsg::DeleteAgent { workspace_uuid, agent_uuid ; reply } => self.delete_agent(&workspace_uuid, &agent_uuid),
                StorageMsg::SetAgentAutoLoop { workspace_uuid, agent_uuid, enabled ; reply } => self.set_agent_auto_loop(&workspace_uuid, &agent_uuid, enabled),
                StorageMsg::UpdateAgent { request ; reply } => self.update_agent(request),
                StorageMsg::AppendAgentUnits { workspace_uuid, agent_uuid, units ; reply } => self.append_agent_units(&workspace_uuid, &agent_uuid, &units),
                StorageMsg::ListUnits { workspace_uuid ; reply } => self.list_units(&workspace_uuid),
                StorageMsg::ReadUnits { workspace_uuid, uuids ; reply } => self.read_units(&workspace_uuid, uuids),
                StorageMsg::WriteUnits { workspace_uuid, units ; reply } => self.write_units(&workspace_uuid, &units),
                StorageMsg::ListContexts { workspace_uuid ; reply } => self.list_contexts(&workspace_uuid),
                StorageMsg::ReadContexts { workspace_uuid, uuids ; reply } => self.read_contexts(&workspace_uuid, uuids),
                StorageMsg::CreateContext { request ; reply } => self.create_context(request),
                StorageMsg::ListWorkflows { workspace_uuid ; reply } => self.list_workflows(&workspace_uuid),
                StorageMsg::ReadWorkflow { workspace_uuid, uuid ; reply } => self.read_workflow(&workspace_uuid, &uuid),
                StorageMsg::CreateWorkflow { request ; reply } => self.create_workflow(request),
                StorageMsg::ListMisc { workspace_uuid ; reply } => self.list_misc(&workspace_uuid),
                StorageMsg::ReadMisc { workspace_uuid, names ; reply } => self.read_misc_entries(&workspace_uuid, names),
                StorageMsg::WriteMisc { workspace_uuid, entries ; reply } => self.write_misc_entries(&workspace_uuid, &entries),
                StorageMsg::ReplaceMisc { workspace_uuid, entries ; reply } => self.replace_misc_entries(&workspace_uuid, entries)
            );
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

    fn create_agent(
        &self,
        request: AgentCreateRequest,
        auto_loop: bool,
        auto_loop_message: String,
    ) -> SubsystemResult<Agent> {
        let name = non_empty_string(request.name, "agent name")?;
        let profile = non_empty_string(request.profile, "agent profile")?;
        let auto_loop_message = if auto_loop {
            non_empty_string(auto_loop_message, "auto_loop_message")?
        } else {
            auto_loop_message
        };
        let now = chrono::Utc::now().timestamp();
        let agent = Agent {
            uuid: safe_object_name(request.uuid, "agent uuid")?,
            name,
            profile,
            auto_loop,
            auto_loop_message,
            unit_chain: Vec::new(),
            unit_head: String::new(),
            context_refs: request.context_refs,
            context_out: request.context_out,
            snapshots: HashMap::new(),
            created_at: now,
            updated_at: now,
        };
        write_json_create_only(
            &self.agent_path(&request.workspace_uuid, &agent.uuid)?,
            &agent,
        )?;
        Ok(agent)
    }

    fn delete_agent(&self, workspace_uuid: &str, agent_uuid: &str) -> SubsystemResult<()> {
        let path = self.agent_path(workspace_uuid, agent_uuid)?;
        if !path.exists() {
            return Err(SubsystemError::not_found("agent", agent_uuid));
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    fn set_agent_auto_loop(
        &self,
        workspace_uuid: &str,
        agent_uuid: &str,
        enabled: bool,
    ) -> SubsystemResult<Agent> {
        let agent_path = self.agent_path(workspace_uuid, agent_uuid)?;
        let old_data = std::fs::read(&agent_path)?;
        let mut agent: Agent = serde_json::from_slice(&old_data).map_err(|error| {
            SubsystemError::invalid_input(format!("{}: {error}", agent_path.display()))
        })?;
        agent.auto_loop = enabled;
        agent.updated_at = chrono::Utc::now().timestamp();
        let new_data = to_pretty_json_vec(&agent)?;
        atomic_replace_file(&agent_path, &old_data, &new_data)?;
        Ok(agent)
    }

    fn update_agent(&self, request: AgentUpdateRequest) -> SubsystemResult<Agent> {
        let agent_path = self.agent_path(&request.workspace_uuid, &request.agent_uuid)?;
        let old_data = std::fs::read(&agent_path)?;
        let mut agent: Agent = serde_json::from_slice(&old_data).map_err(|error| {
            SubsystemError::invalid_input(format!("{}: {error}", agent_path.display()))
        })?;
        if let Some(context_refs) = request.context_refs {
            validate_object_names(&context_refs, "context_refs")?;
            agent.context_refs = context_refs;
        }
        if let Some(context_out) = request.context_out {
            validate_object_names(&context_out, "context_out")?;
            agent.context_out = context_out;
        }
        if let Some(auto_loop) = request.auto_loop {
            agent.auto_loop = auto_loop;
        }
        if let Some(auto_loop_message) = request.auto_loop_message {
            agent.auto_loop_message = non_empty_string(auto_loop_message, "auto_loop_message")?;
        }
        agent.updated_at = chrono::Utc::now().timestamp();
        let new_data = to_pretty_json_vec(&agent)?;
        atomic_replace_file(&agent_path, &old_data, &new_data)?;
        Ok(agent)
    }

    fn append_agent_units(
        &self,
        workspace_uuid: &str,
        agent_uuid: &str,
        units: &[Unit],
    ) -> SubsystemResult<Agent> {
        let agent_path = self.agent_path(workspace_uuid, agent_uuid)?;
        let old_data = std::fs::read(&agent_path)?;
        let mut agent: Agent = serde_json::from_slice(&old_data).map_err(|error| {
            SubsystemError::invalid_input(format!("{}: {error}", agent_path.display()))
        })?;
        for unit in units {
            write_json_create_only(&self.unit_path(workspace_uuid, &unit.uuid)?, unit)?;
            agent.unit_chain.push(unit.uuid.clone());
            agent.unit_head = unit.uuid.clone();
        }
        agent.updated_at = chrono::Utc::now().timestamp();
        let new_data = to_pretty_json_vec(&agent)?;
        atomic_replace_file(&agent_path, &old_data, &new_data)?;
        Ok(agent)
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

    fn create_context(&self, request: ContextCreateRequest) -> SubsystemResult<Context> {
        let context = Context {
            uuid: safe_object_name(request.uuid, "context uuid")?,
            title: non_empty_string(request.title, "context title")?,
            content: non_empty_string(request.content, "context content")?,
            created_at: chrono::Utc::now().timestamp(),
        };
        write_json_create_only(
            &self.context_path(&request.workspace_uuid, &context.uuid)?,
            &context,
        )?;
        Ok(context)
    }

    fn list_workflows(&self, workspace_uuid: &str) -> SubsystemResult<Vec<String>> {
        list_json_object_ids(&self.workspace_root(workspace_uuid)?.join("workflows"))
    }

    fn read_workflow(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<Workflow> {
        read_json(&self.workflow_path(workspace_uuid, uuid)?)
    }

    fn create_workflow(&self, request: WorkflowCreateRequest) -> SubsystemResult<Workflow> {
        let now = chrono::Utc::now().timestamp();
        let workflow = Workflow {
            uuid: safe_object_name(request.uuid, "workflow uuid")?,
            title: non_empty_string(request.title, "workflow title")?,
            content: non_empty_string(request.content, "workflow content")?,
            metadata: request.metadata,
            created_at: now,
            updated_at: now,
        };
        write_json_create_only(
            &self.workflow_path(&request.workspace_uuid, &workflow.uuid)?,
            &workflow,
        )?;
        Ok(workflow)
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
        validate_object_name(uuid, "agent uuid")?;
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("agents")
            .join(format!("{uuid}.json")))
    }

    fn unit_path(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<PathBuf> {
        validate_object_name(uuid, "unit uuid")?;
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("units")
            .join(format!("{uuid}.json")))
    }

    fn context_path(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<PathBuf> {
        validate_object_name(uuid, "context uuid")?;
        Ok(self
            .workspace_root(workspace_uuid)?
            .join("contexts")
            .join(format!("{uuid}.json")))
    }

    fn workflow_path(&self, workspace_uuid: &str, uuid: &str) -> SubsystemResult<PathBuf> {
        validate_object_name(uuid, "workflow uuid")?;
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

// ---- Declarative macro: handle methods with concrete types ----

impl_handle_methods! {
    StorageHandle for StorageMsg, STORAGE_ACTOR;

    fn root(&self) -> PathBuf
        => Root {};

    fn create_agent(&self, request: AgentCreateRequest, auto_loop: bool, auto_loop_message: String) -> Agent
        => CreateAgent { request: request, auto_loop: auto_loop, auto_loop_message: auto_loop_message };

    fn update_agent(&self, request: AgentUpdateRequest) -> Agent
        => UpdateAgent { request: request };

    fn create_context(&self, request: ContextCreateRequest) -> Context
        => CreateContext { request: request };

    fn create_workflow(&self, request: WorkflowCreateRequest) -> Workflow
        => CreateWorkflow { request: request };
}

// ---- Manual handle methods (impl Into<String> for API compatibility) ----

impl StorageHandle {
    pub async fn list_agents(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ListAgents {
                workspace_uuid: w,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn read_agents(
        &self,
        workspace_uuid: impl Into<String>,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Agent>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ReadAgents {
                workspace_uuid: w,
                uuids,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn write_agents(
        &self,
        workspace_uuid: impl Into<String>,
        agents: Vec<Agent>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::WriteAgents {
                workspace_uuid: w,
                agents,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn delete_agent(
        &self,
        workspace_uuid: impl Into<String>,
        agent_uuid: impl Into<String>,
    ) -> SubsystemResult<()> {
        let w = workspace_uuid.into();
        let a = agent_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::DeleteAgent {
                workspace_uuid: w,
                agent_uuid: a,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn set_agent_auto_loop(
        &self,
        workspace_uuid: impl Into<String>,
        agent_uuid: impl Into<String>,
        enabled: bool,
    ) -> SubsystemResult<Agent> {
        let w = workspace_uuid.into();
        let a = agent_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::SetAgentAutoLoop {
                workspace_uuid: w,
                agent_uuid: a,
                enabled,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn append_agent_units(
        &self,
        workspace_uuid: impl Into<String>,
        agent_uuid: impl Into<String>,
        units: Vec<Unit>,
    ) -> SubsystemResult<Agent> {
        let w = workspace_uuid.into();
        let a = agent_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::AppendAgentUnits {
                workspace_uuid: w,
                agent_uuid: a,
                units,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn list_units(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ListUnits {
                workspace_uuid: w,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn read_units(
        &self,
        workspace_uuid: impl Into<String>,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Unit>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ReadUnits {
                workspace_uuid: w,
                uuids,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn write_units(
        &self,
        workspace_uuid: impl Into<String>,
        units: Vec<Unit>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::WriteUnits {
                workspace_uuid: w,
                units,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn list_contexts(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ListContexts {
                workspace_uuid: w,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn read_contexts(
        &self,
        workspace_uuid: impl Into<String>,
        uuids: Vec<String>,
    ) -> SubsystemResult<Vec<Context>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ReadContexts {
                workspace_uuid: w,
                uuids,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn list_workflows(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ListWorkflows {
                workspace_uuid: w,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn read_workflow(
        &self,
        workspace_uuid: impl Into<String>,
        uuid: impl Into<String>,
    ) -> SubsystemResult<Workflow> {
        let w = workspace_uuid.into();
        let u = uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ReadWorkflow {
                workspace_uuid: w,
                uuid: u,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn list_misc(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ListMisc {
                workspace_uuid: w,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn read_misc(
        &self,
        workspace_uuid: impl Into<String>,
        names: Vec<String>,
    ) -> SubsystemResult<Vec<MiscReadEntry>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ReadMisc {
                workspace_uuid: w,
                names,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn write_misc(
        &self,
        workspace_uuid: impl Into<String>,
        entries: Vec<MiscWriteEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::WriteMisc {
                workspace_uuid: w,
                entries,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
    }

    pub async fn replace_misc(
        &self,
        workspace_uuid: impl Into<String>,
        entries: Vec<MiscReplaceEntry>,
    ) -> SubsystemResult<Vec<String>> {
        let w = workspace_uuid.into();
        crate::macros::_request(
            &self.tx,
            |reply| StorageMsg::ReplaceMisc {
                workspace_uuid: w,
                entries,
                reply,
            },
            STORAGE_ACTOR,
        )
        .await
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

fn non_empty_string(value: String, field: &'static str) -> SubsystemResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(SubsystemError::invalid_input(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(trimmed.to_string())
    }
}

fn safe_object_name(value: String, field: &'static str) -> SubsystemResult<String> {
    let value = non_empty_string(value, field)?;
    validate_object_name(&value, field)?;
    Ok(value)
}

fn validate_object_name(value: &str, field: &'static str) -> SubsystemResult<()> {
    if is_safe_object_name(value) {
        Ok(())
    } else {
        Err(SubsystemError::invalid_input(format!(
            "invalid {field}: {value}"
        )))
    }
}

fn validate_object_names(values: &[String], field: &'static str) -> SubsystemResult<()> {
    for value in values {
        validate_object_name(value, field)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_actor() -> StorageActor {
        let (_tx, rx) = mpsc::channel(1);
        let root = std::env::temp_dir().join(format!("prismagent-storage-{}", Uuid::now_v7()));
        let actor = StorageActor::from_root(rx, root).unwrap();
        std::fs::create_dir_all(actor.root.join("workspace-1").join("agents")).unwrap();
        actor
    }

    fn create_request(uuid: &str) -> AgentCreateRequest {
        AgentCreateRequest {
            workspace_uuid: "workspace-1".to_string(),
            uuid: uuid.to_string(),
            name: "Test agent".to_string(),
            profile: "default".to_string(),
            context_refs: Vec::new(),
            context_out: Vec::new(),
        }
    }

    #[test]
    fn create_agent_allows_empty_auto_loop_message_when_auto_loop_is_false() {
        let actor = test_actor();
        let agent = actor
            .create_agent(create_request("agent-1"), false, String::new())
            .unwrap();

        assert!(!agent.auto_loop);
        assert_eq!(agent.auto_loop_message, "");
    }

    #[test]
    fn create_agent_rejects_empty_auto_loop_message_when_auto_loop_is_true() {
        let actor = test_actor();
        let error = actor
            .create_agent(create_request("agent-1"), true, String::new())
            .unwrap_err();

        match error {
            SubsystemError::InvalidInput { message } => {
                assert_eq!(message, "auto_loop_message must not be empty");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }
}
