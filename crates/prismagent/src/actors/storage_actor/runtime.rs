use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest, AgentUpdateRequest};
use crate::actors::storage_actor::model::context::{Context, ContextCreateRequest};
use crate::actors::storage_actor::model::misc::{MiscReadEntry, MiscReplaceEntry, MiscWriteEntry};
use crate::actors::storage_actor::model::unit::Unit;
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowCreateRequest};
use crate::actors::storage_actor::model::{STORAGE_ACTOR, StorageActor, StorageHandle, StorageMsg};
use crate::error::{ConflictKind, ResourceKind, SubsystemError, SubsystemResult};
use crate::id::AgentAddr;
use crate::{actor_dispatch, impl_handle_methods};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

impl StorageActor {
    pub fn load(rx: mpsc::Receiver<StorageMsg>) -> SubsystemResult<Self> {
        let root = dirs::home_dir()
            .ok_or_else(|| {
                SubsystemError::internal(
                    "resolve storage directory",
                    "home directory is unavailable",
                )
            })?
            .join(".prismagent")
            .join("workspaces");
        Self::from_root(rx, root)
    }

    pub fn from_root(rx: mpsc::Receiver<StorageMsg>, root: PathBuf) -> SubsystemResult<Self> {
        io_result(
            "create storage directory",
            &root,
            std::fs::create_dir_all(&root),
        )?;
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
                StorageMsg::ReadAgent { addr ; reply } => self.read_agent(&addr),
                StorageMsg::ReadAgents { workspace_uuid, uuids ; reply } => self.read_agents(&workspace_uuid, uuids),
                StorageMsg::CreateAgent { request, auto_loop, auto_loop_message ; reply } => self.create_agent(request, auto_loop, auto_loop_message),
                StorageMsg::DeleteAgent { addr ; reply } => self.delete_agent(&addr),
                StorageMsg::SetAgentAutoLoop { addr, enabled ; reply } => self.set_agent_auto_loop(&addr, enabled),
                StorageMsg::UpdateAgent { request ; reply } => self.update_agent(request),
                StorageMsg::AppendAgentUnits { addr, units ; reply } => self.append_agent_units(&addr, &units),
                StorageMsg::ListUnits { workspace_uuid ; reply } => self.list_units(&workspace_uuid),
                StorageMsg::ReadUnits { workspace_uuid, uuids ; reply } => self.read_units(&workspace_uuid, uuids),
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

    fn read_agent(&self, addr: &AgentAddr) -> SubsystemResult<Agent> {
        read_agent_file(&self.agent_path(addr)?, addr).map(|(agent, _)| agent)
    }

    fn read_agents(&self, workspace_uuid: &str, uuids: Vec<String>) -> SubsystemResult<Vec<Agent>> {
        uuids
            .iter()
            .map(|uuid| self.read_agent(&AgentAddr::new(workspace_uuid, uuid)))
            .collect()
    }

    fn create_agent(
        &self,
        mut request: AgentCreateRequest,
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
        request.addr.agent_uuid = safe_object_name(request.addr.agent_uuid, "agent uuid")?;
        let path = self.agent_path(&request.addr)?;
        let agent = Agent {
            uuid: request.addr.agent_uuid,
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
        write_json_create_only(&path, &agent)?;
        Ok(agent)
    }

    fn delete_agent(&self, addr: &AgentAddr) -> SubsystemResult<()> {
        let path = self.agent_path(addr)?;
        if !path.exists() {
            return Err(SubsystemError::not_found(
                ResourceKind::Agent,
                &addr.agent_uuid,
            ));
        }
        io_result("delete agent file", &path, std::fs::remove_file(&path))?;
        Ok(())
    }

    fn edit_agent(
        &self,
        addr: &AgentAddr,
        edit: impl FnOnce(&mut Agent) -> SubsystemResult<()>,
    ) -> SubsystemResult<Agent> {
        let agent_path = self.agent_path(addr)?;
        let (mut agent, old_data) = read_agent_file(&agent_path, addr)?;
        edit(&mut agent)?;
        agent.updated_at = chrono::Utc::now().timestamp();
        let new_data = to_pretty_json_vec(&agent)?;
        atomic_replace_file(&agent_path, &old_data, &new_data)?;
        Ok(agent)
    }

    fn set_agent_auto_loop(&self, addr: &AgentAddr, enabled: bool) -> SubsystemResult<Agent> {
        self.edit_agent(addr, |agent| {
            agent.auto_loop = enabled;
            Ok(())
        })
    }

    fn update_agent(&self, request: AgentUpdateRequest) -> SubsystemResult<Agent> {
        self.edit_agent(&request.addr, |agent| {
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
            Ok(())
        })
    }

    fn append_agent_units(&self, addr: &AgentAddr, units: &[Unit]) -> SubsystemResult<Agent> {
        self.edit_agent(addr, |agent| {
            for unit in units {
                write_json_create_only(&self.unit_path(&addr.workspace_uuid, &unit.uuid)?, unit)?;
                agent.unit_chain.push(unit.uuid.clone());
                agent.unit_head = unit.uuid.clone();
            }
            Ok(())
        })
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
            return Err(SubsystemError::validation_field(
                "workspace uuid",
                format!("invalid workspace uuid: {workspace_uuid}"),
            ));
        }
        let root = self.root.join(workspace_uuid);
        if !root.is_dir() {
            return Err(SubsystemError::not_found(
                ResourceKind::Workspace,
                workspace_uuid,
            ));
        }
        Ok(root)
    }

    fn agent_path(&self, addr: &AgentAddr) -> SubsystemResult<PathBuf> {
        validate_object_name(&addr.agent_uuid, "agent uuid")?;
        Ok(self
            .workspace_root(&addr.workspace_uuid)?
            .join("agents")
            .join(format!("{}.json", addr.agent_uuid)))
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
            return Err(SubsystemError::validation_field(
                "misc name",
                format!("invalid misc name: {name}"),
            ));
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

    fn list_agents(&self, workspace_uuid: impl Into<String>) -> Vec<String>
        => ListAgents { workspace_uuid: workspace_uuid.into() };

    fn read_agent(&self, addr: AgentAddr) -> Agent
        => ReadAgent { addr: addr };

    fn read_agents(&self, workspace_uuid: impl Into<String>, uuids: Vec<String>) -> Vec<Agent>
        => ReadAgents { workspace_uuid: workspace_uuid.into(), uuids: uuids };

    fn create_agent(&self, request: AgentCreateRequest, auto_loop: bool, auto_loop_message: String) -> Agent
        => CreateAgent { request: request, auto_loop: auto_loop, auto_loop_message: auto_loop_message };

    fn update_agent(&self, request: AgentUpdateRequest) -> Agent
        => UpdateAgent { request: request };

    fn append_agent_units(&self, addr: AgentAddr, units: Vec<Unit>) -> Agent
        => AppendAgentUnits { addr: addr, units: units };

    fn set_agent_auto_loop(&self, addr: AgentAddr, enabled: bool) -> Agent
        => SetAgentAutoLoop { addr: addr, enabled: enabled };

    fn delete_agent(&self, addr: AgentAddr) -> ()
        => DeleteAgent { addr: addr };

    fn list_contexts(&self, workspace_uuid: impl Into<String>) -> Vec<String>
        => ListContexts { workspace_uuid: workspace_uuid.into() };

    fn read_contexts(&self, workspace_uuid: impl Into<String>, uuids: Vec<String>) -> Vec<Context>
        => ReadContexts { workspace_uuid: workspace_uuid.into(), uuids: uuids };

    fn create_context(&self, request: ContextCreateRequest) -> Context
        => CreateContext { request: request };

    // never used !
    fn list_workflows(&self, workspace_uuid: impl Into<String>) -> Vec<String>
        => ListWorkflows { workspace_uuid: workspace_uuid.into() };

    fn create_workflow(&self, request: WorkflowCreateRequest) -> Workflow
        => CreateWorkflow { request: request };

    fn read_workflow(&self, workspace_uuid: impl Into<String>, uuid: impl Into<String>) -> Workflow
        => ReadWorkflow { workspace_uuid: workspace_uuid.into(), uuid: uuid.into() };

    // never used !
    fn list_units(&self, workspace_uuid: impl Into<String>) -> Vec<String>
        => ListUnits { workspace_uuid: workspace_uuid.into() };

    fn read_units(&self, workspace_uuid: impl Into<String>, uuids: Vec<String>) -> Vec<Unit>
        => ReadUnits { workspace_uuid: workspace_uuid.into(), uuids: uuids };

    // never used !
    fn list_misc(&self, workspace_uuid: impl Into<String>) -> Vec<String>
        => ListMisc { workspace_uuid: workspace_uuid.into() };

    // never used !
    fn read_misc(&self, workspace_uuid: impl Into<String>, names: Vec<String>) -> Vec<MiscReadEntry>
        => ReadMisc { workspace_uuid: workspace_uuid.into(), names: names };

    // never used !
    fn write_misc(&self, workspace_uuid: impl Into<String>, entries: Vec<MiscWriteEntry>) -> Vec<String>
        => WriteMisc { workspace_uuid: workspace_uuid.into(), entries: entries };

    // never used !
    fn replace_misc(&self, workspace_uuid: impl Into<String>, entries: Vec<MiscReplaceEntry>) -> Vec<String>
        => ReplaceMisc { workspace_uuid: workspace_uuid.into(), entries: entries };
}

fn io_result<T>(
    operation: &'static str,
    path: &Path,
    result: std::io::Result<T>,
) -> SubsystemResult<T> {
    result.map_err(|error| SubsystemError::io(operation, Some(path.to_path_buf()), error))
}

fn atomic_create_file(dst: &Path, data: &[u8]) -> SubsystemResult<()> {
    if dst.exists() {
        return Err(SubsystemError::conflict(
            ConflictKind::FileAlreadyExists,
            dst.display().to_string(),
        ));
    }
    let parent = dst.parent().ok_or_else(|| {
        SubsystemError::internal(
            "create storage file",
            format!("path has no parent directory: {}", dst.display()),
        )
    })?;
    io_result(
        "create storage parent directory",
        parent,
        std::fs::create_dir_all(parent),
    )?;
    let tmp_dst = dst.with_extension("tmp");
    io_result(
        "write temporary storage file",
        &tmp_dst,
        std::fs::write(&tmp_dst, data),
    )?;
    io_result("commit storage file", dst, std::fs::rename(&tmp_dst, dst))?;
    Ok(())
}

fn atomic_replace_file(dst: &Path, old: &[u8], new: &[u8]) -> SubsystemResult<()> {
    if !dst.exists() {
        return Err(SubsystemError::not_found(
            ResourceKind::File,
            dst.display().to_string(),
        ));
    }
    let current_data = io_result("read storage file", dst, std::fs::read(dst))?;
    if current_data != old {
        return Err(SubsystemError::conflict(
            ConflictKind::ConcurrentModification,
            dst.display().to_string(),
        ));
    }
    let tmp_dst = dst.with_extension("tmp");
    io_result(
        "write temporary storage file",
        &tmp_dst,
        std::fs::write(&tmp_dst, new),
    )?;
    io_result("commit storage file", dst, std::fs::rename(&tmp_dst, dst))?;
    Ok(())
}

// Keep the original bytes for the compare-and-replace check on writes.
fn read_agent_file(path: &Path, addr: &AgentAddr) -> SubsystemResult<(Agent, Vec<u8>)> {
    let read_error = |error: std::io::Error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            SubsystemError::not_found(ResourceKind::Agent, addr.to_string())
        }
        _ => SubsystemError::io("read agent file", Some(path.to_owned()), error),
    };
    if !std::fs::metadata(path).map_err(read_error)?.is_file() {
        return Err(SubsystemError::corrupt_state(
            "agent file",
            format!("{addr} is not a regular file"),
        ));
    }
    let bytes = std::fs::read(path).map_err(read_error)?;
    let agent: Agent = serde_json::from_slice(&bytes).map_err(|error| {
        SubsystemError::corrupt_state("agent file", format!("{}: {error}", path.display()))
    })?;
    if agent.uuid != addr.agent_uuid {
        return Err(SubsystemError::corrupt_state(
            "agent file",
            format!("agent ID does not match {addr}"),
        ));
    }
    Ok((agent, bytes))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> SubsystemResult<T> {
    if !path.is_file() {
        return Err(SubsystemError::not_found(
            ResourceKind::File,
            path.display().to_string(),
        ));
    }
    let data = io_result("read storage file", path, std::fs::read(path))?;
    serde_json::from_slice(&data).map_err(|error| {
        SubsystemError::corrupt_state("storage JSON file", format!("{}: {error}", path.display()))
    })
}

fn write_json_create_only<T: serde::Serialize>(path: &Path, value: &T) -> SubsystemResult<()> {
    let data = to_pretty_json_vec(value)?;
    atomic_create_file(path, &data)
}

fn to_pretty_json_vec<T: serde::Serialize>(value: &T) -> SubsystemResult<Vec<u8>> {
    let mut data = Vec::new();
    serde_json::to_writer_pretty(&mut data, value)
        .map_err(|error| SubsystemError::internal("serialize storage object", error.to_string()))?;
    data.push(b'\n');
    Ok(data)
}

fn list_json_object_ids(dir: &Path) -> SubsystemResult<Vec<String>> {
    let mut ids = Vec::new();
    if !dir.exists() {
        return Ok(ids);
    }
    let entries = io_result("list storage directory", dir, std::fs::read_dir(dir))?;
    for entry in entries {
        let path = entry
            .map_err(|error| {
                SubsystemError::io(
                    "read storage directory entry",
                    Some(dir.to_path_buf()),
                    error,
                )
            })?
            .path();
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
        Err(SubsystemError::validation_field(
            field,
            format!("{field} must not be empty"),
        ))
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
        Err(SubsystemError::validation_field(
            field,
            format!("invalid {field}: {value}"),
        ))
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
            addr: AgentAddr::new("workspace-1", uuid),
            name: "Test agent".to_string(),
            profile: "default".to_string(),
            context_refs: Vec::new(),
            context_out: Vec::new(),
        }
    }

    #[test]
    fn mismatched_agent_file_is_never_modified() {
        let actor = test_actor();
        actor
            .create_agent(create_request("wrong"), false, String::new())
            .unwrap();
        let addr = AgentAddr::new("workspace-1", "target");
        let path = actor.agent_path(&addr).unwrap();
        std::fs::rename(path.with_file_name("wrong.json"), &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(matches!(
            actor.set_agent_auto_loop(&addr, true),
            Err(SubsystemError::CorruptState { .. })
        ));
        assert_eq!(std::fs::read(path).unwrap(), original);
        std::fs::remove_dir_all(actor.root).unwrap();
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
            SubsystemError::Validation { message, .. } => {
                assert_eq!(message, "auto_loop_message must not be empty");
            }
            other => panic!("expected Validation, got {other:?}"),
        }
    }
}
