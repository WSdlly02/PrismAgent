use crate::actors::workspace_actor::model::{
    WORKSPACE_ACTOR, Workspace, WorkspaceActor, WorkspaceCreateRequest, WorkspaceHandle,
    WorkspaceMsg, WorkspaceSummary,
};
use crate::error::{SubsystemError, SubsystemResult};
use crate::{actor_dispatch, impl_handle_methods};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use uuid::Uuid;

impl WorkspaceActor {
    pub fn load(rx: mpsc::Receiver<WorkspaceMsg>) -> SubsystemResult<Self> {
        let root = dirs::home_dir()
            .ok_or_else(|| SubsystemError::internal("failed to determine home directory"))?
            .join(".prismagent")
            .join("workspaces");
        Self::from_root(rx, root)
    }

    pub fn from_root(rx: mpsc::Receiver<WorkspaceMsg>, root: PathBuf) -> SubsystemResult<Self> {
        std::fs::create_dir_all(&root)?;
        Ok(Self {
            rx,
            root,
            workspaces: HashMap::new(),
        })
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            actor_dispatch!(msg;
                WorkspaceMsg::List { ; reply } => self.list().await,
                WorkspaceMsg::Create { request ; reply } => self.create(request).await,
                WorkspaceMsg::Contains { workspace_uuid ; reply } => self.contains(workspace_uuid).await,
                WorkspaceMsg::Get { workspace_uuid ; reply } => self.get(workspace_uuid).await,
            );
        }
    }

    async fn list(&mut self) -> SubsystemResult<Vec<WorkspaceSummary>> {
        for entry in std::fs::read_dir(&self.root)? {
            let path = entry?.path();
            let metadata_path = path.join("metadata.json");
            if !path.is_dir() || !metadata_path.is_file() {
                continue;
            }
            let workspace = read_workspace(&metadata_path)?;
            self.workspaces
                .entry(workspace.uuid.clone())
                .or_insert(workspace);
        }
        let mut workspaces = self
            .workspaces
            .values()
            .map(|workspace| WorkspaceSummary {
                workspace_uuid: workspace.uuid.clone(),
                workspace_path: workspace.path.clone(),
                locked_by: None,
            })
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| left.workspace_path.cmp(&right.workspace_path));
        Ok(workspaces)
    }

    async fn create(
        &mut self,
        request: WorkspaceCreateRequest,
    ) -> SubsystemResult<WorkspaceSummary> {
        let workspace_path = std::fs::canonicalize(&request.path)?;
        if !workspace_path.is_dir() {
            return Err(SubsystemError::invalid_input(format!(
                "workspace path is not a directory: {}",
                workspace_path.display()
            )));
        }
        let existing = self.list().await?;
        if existing
            .iter()
            .any(|workspace| workspace.workspace_path == workspace_path)
        {
            return Err(SubsystemError::Conflict {
                resource: "workspace_path",
                id: workspace_path.display().to_string(),
            });
        }
        let uuid = Uuid::now_v7().to_string();
        let workspace_root = self.root.join(&uuid);
        std::fs::create_dir(&workspace_root)?;
        for child in ["agents", "units", "contexts", "workflows", "misc", "skills"] {
            std::fs::create_dir_all(workspace_root.join(child))?;
        }
        let workspace = Workspace {
            uuid,
            path: workspace_path,
        };
        write_workspace(&workspace_root.join("metadata.json"), &workspace)?;
        self.workspaces
            .insert(workspace.uuid.clone(), workspace.clone());
        Ok(WorkspaceSummary {
            workspace_uuid: workspace.uuid,
            workspace_path: workspace.path,
            locked_by: None,
        })
    }

    async fn contains(&self, workspace_uuid: String) -> SubsystemResult<bool> {
        Ok(self.workspaces.contains_key(&workspace_uuid))
    }

    async fn get(&self, workspace_uuid: String) -> SubsystemResult<Workspace> {
        self.workspaces
            .get(&workspace_uuid)
            .cloned()
            .ok_or_else(|| SubsystemError::not_found("workspace", &workspace_uuid))
    }
}

fn write_workspace(path: &std::path::Path, workspace: &Workspace) -> SubsystemResult<()> {
    let data = serde_json::to_vec_pretty(workspace)
        .map_err(|error| SubsystemError::invalid_input(error.to_string()))?;
    std::fs::write(path, data)?;
    Ok(())
}

fn read_workspace(path: &std::path::Path) -> SubsystemResult<Workspace> {
    let workspace: Workspace = serde_json::from_slice(&std::fs::read(path)?)
        .map_err(|error| SubsystemError::invalid_input(format!("{}: {error}", path.display())))?;
    let directory_uuid = path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            SubsystemError::invalid_input(format!(
                "{}: invalid workspace directory",
                path.display()
            ))
        })?;
    if workspace.uuid != directory_uuid {
        return Err(SubsystemError::invalid_input(format!(
            "{}: uuid {} does not match directory {directory_uuid}",
            path.display(),
            workspace.uuid
        )));
    }
    Ok(workspace)
}

// ---- Declarative macros: handle methods (list, create via macro; contains, get manual) ----

impl_handle_methods! {
    WorkspaceHandle for WorkspaceMsg, WORKSPACE_ACTOR;

    fn list(&self) -> Vec<WorkspaceSummary>
        => List {};

    fn get(&self, workspace_uuid: impl Into<String>) -> Workspace
        => Get { workspace_uuid: workspace_uuid.into() };

    fn contains(&self, workspace_uuid: impl Into<String>) -> bool
        => Contains { workspace_uuid: workspace_uuid.into() };

    fn create(&self, request: WorkspaceCreateRequest) -> WorkspaceSummary
        => Create { request: request };

    fn delete(&self, workspace_uuid: impl Into<String>) -> ()
        => Delete { workspace_uuid: workspace_uuid.into() };
}
