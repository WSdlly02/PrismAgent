use crate::actors::workspace_actor::model::{
    WORKSPACE_ACTOR, Workspace, WorkspaceActor, WorkspaceCreateRequest, WorkspaceHandle,
    WorkspaceMsg, WorkspaceSummary,
};
use crate::error::{ConflictKind, ResourceKind, SubsystemError, SubsystemResult};
use crate::{actor_dispatch, impl_handle_methods};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use uuid::Uuid;

impl WorkspaceActor {
    pub fn load(rx: mpsc::Receiver<WorkspaceMsg>) -> SubsystemResult<Self> {
        let root = dirs::home_dir()
            .ok_or_else(|| {
                SubsystemError::internal(
                    "resolve workspace metadata directory",
                    "home directory is unavailable",
                )
            })?
            .join(".prismagent")
            .join("workspaces");
        Self::from_root(rx, root)
    }

    pub fn from_root(rx: mpsc::Receiver<WorkspaceMsg>, root: PathBuf) -> SubsystemResult<Self> {
        std::fs::create_dir_all(&root).map_err(|error| {
            SubsystemError::io(
                "create workspace metadata directory",
                Some(root.clone()),
                error,
            )
        })?;
        let trash_root = root.join(".trash");
        std::fs::create_dir_all(&trash_root).map_err(|error| {
            SubsystemError::io(
                "create workspace trash directory",
                Some(trash_root.clone()),
                error,
            )
        })?;
        cleanup_workspace_trash(&trash_root);
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
                WorkspaceMsg::Delete { workspace_uuid ; reply } => self.delete(workspace_uuid).await,
            );
        }
    }

    async fn list(&mut self) -> SubsystemResult<Vec<WorkspaceSummary>> {
        let entries = std::fs::read_dir(&self.root).map_err(|error| {
            SubsystemError::io(
                "list workspace metadata directory",
                Some(self.root.clone()),
                error,
            )
        })?;
        for entry in entries {
            let path = entry
                .map_err(|error| {
                    SubsystemError::io(
                        "read workspace directory entry",
                        Some(self.root.clone()),
                        error,
                    )
                })?
                .path();
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
        let workspace_path = std::fs::canonicalize(&request.path).map_err(|error| {
            SubsystemError::io("resolve workspace path", Some(request.path.clone()), error)
        })?;
        if !workspace_path.is_dir() {
            return Err(SubsystemError::validation_field(
                "workspace path",
                format!(
                    "workspace path is not a directory: {}",
                    workspace_path.display()
                ),
            ));
        }
        let existing = self.list().await?;
        if existing
            .iter()
            .any(|workspace| workspace.workspace_path == workspace_path)
        {
            return Err(SubsystemError::conflict(
                ConflictKind::WorkspacePathExists,
                workspace_path.display().to_string(),
            ));
        }
        let uuid = Uuid::now_v7().to_string();
        let workspace_root = self.root.join(&uuid);
        std::fs::create_dir(&workspace_root).map_err(|error| {
            SubsystemError::io(
                "create workspace metadata",
                Some(workspace_root.clone()),
                error,
            )
        })?;
        for child in ["agents", "units", "contexts", "workflows", "misc", "skills"] {
            let child_path = workspace_root.join(child);
            std::fs::create_dir_all(&child_path).map_err(|error| {
                SubsystemError::io("create workspace data directory", Some(child_path), error)
            })?;
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
            .ok_or_else(|| SubsystemError::not_found(ResourceKind::Workspace, &workspace_uuid))
    }

    async fn delete(&mut self, workspace_uuid: String) -> SubsystemResult<()> {
        let path = self.root.join(&workspace_uuid);
        if !path.exists() {
            return Err(SubsystemError::not_found(
                ResourceKind::Workspace,
                &workspace_uuid,
            ));
        }

        // Renaming within the same filesystem is the logical commit point:
        // before it succeeds the active workspace is untouched; afterwards it
        // is no longer discoverable even if physical cleanup later fails.
        let trash_root = self.root.join(".trash");
        std::fs::create_dir_all(&trash_root).map_err(|error| {
            SubsystemError::io(
                "create workspace trash directory",
                Some(trash_root.clone()),
                error,
            )
        })?;
        let trashed_path = trash_root.join(format!("{workspace_uuid}-{}", Uuid::now_v7()));
        std::fs::rename(&path, &trashed_path).map_err(|error| {
            SubsystemError::io("commit workspace deletion", Some(path.clone()), error)
        })?;
        self.workspaces.remove(&workspace_uuid);

        tokio::task::spawn_blocking(move || {
            if let Err(error) = std::fs::remove_dir_all(&trashed_path) {
                eprintln!(
                    "failed to clean deleted workspace at {}: {error}",
                    trashed_path.display()
                );
            }
        });
        Ok(())
    }
}

fn cleanup_workspace_trash(trash_root: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(trash_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(error) = result {
            eprintln!(
                "failed to clean deleted workspace at {}: {error}",
                path.display()
            );
        }
    }
}

fn write_workspace(path: &std::path::Path, workspace: &Workspace) -> SubsystemResult<()> {
    let data = serde_json::to_vec_pretty(workspace).map_err(|error| {
        SubsystemError::internal("serialize workspace metadata", error.to_string())
    })?;
    std::fs::write(path, data).map_err(|error| {
        SubsystemError::io("write workspace metadata", Some(path.to_path_buf()), error)
    })?;
    Ok(())
}

fn read_workspace(path: &std::path::Path) -> SubsystemResult<Workspace> {
    let data = std::fs::read(path).map_err(|error| {
        SubsystemError::io("read workspace metadata", Some(path.to_path_buf()), error)
    })?;
    let workspace: Workspace = serde_json::from_slice(&data).map_err(|error| {
        SubsystemError::corrupt_state("workspace metadata", format!("{}: {error}", path.display()))
    })?;
    let directory_uuid = path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            SubsystemError::corrupt_state(
                "workspace metadata",
                format!("{}: invalid workspace directory", path.display()),
            )
        })?;
    if workspace.uuid != directory_uuid {
        return Err(SubsystemError::corrupt_state(
            "workspace metadata",
            format!(
                "{}: uuid {} does not match directory {directory_uuid}",
                path.display(),
                workspace.uuid
            ),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_actor() -> (WorkspaceActor, PathBuf) {
        let root = std::env::temp_dir().join(format!("prismagent-workspaces-{}", Uuid::now_v7()));
        let (_tx, rx) = mpsc::channel(1);
        (WorkspaceActor::from_root(rx, root.clone()).unwrap(), root)
    }

    #[tokio::test]
    async fn failed_delete_keeps_workspace_in_memory() {
        let (mut actor, root) = test_actor();
        let workspace_uuid = Uuid::now_v7().to_string();
        actor.workspaces.insert(
            workspace_uuid.clone(),
            Workspace {
                uuid: workspace_uuid.clone(),
                path: root.join("project"),
            },
        );

        assert!(actor.delete(workspace_uuid.clone()).await.is_err());
        assert!(actor.workspaces.contains_key(&workspace_uuid));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn successful_delete_removes_active_path_before_memory_entry() {
        let (mut actor, root) = test_actor();
        let workspace_uuid = Uuid::now_v7().to_string();
        let active_path = root.join(&workspace_uuid);
        std::fs::create_dir_all(&active_path).unwrap();
        actor.workspaces.insert(
            workspace_uuid.clone(),
            Workspace {
                uuid: workspace_uuid.clone(),
                path: root.join("project"),
            },
        );

        actor.delete(workspace_uuid.clone()).await.unwrap();

        assert!(!active_path.exists());
        assert!(!actor.workspaces.contains_key(&workspace_uuid));
        std::fs::remove_dir_all(root).unwrap();
    }
}
