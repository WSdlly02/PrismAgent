use crate::actors::workspace_actor::model::{
    WORKSPACE_ACTOR, Workspace, WorkspaceActor, WorkspaceHandle, WorkspaceMsg, WorkspaceSummary,
};
use crate::error::{SubsystemError, SubsystemResult};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

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
            match msg {
                WorkspaceMsg::List { reply } => {
                    let _ = reply.send(self.list());
                }
                WorkspaceMsg::Contains {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(Ok(self.workspaces.contains_key(&workspace_uuid)));
                }
                WorkspaceMsg::Get {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(self.get(&workspace_uuid));
                }
            }
        }
    }

    fn list(&mut self) -> SubsystemResult<Vec<WorkspaceSummary>> {
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

    fn get(&self, workspace_uuid: &str) -> SubsystemResult<Workspace> {
        self.workspaces
            .get(workspace_uuid)
            .cloned()
            .ok_or_else(|| SubsystemError::not_found("workspace", workspace_uuid))
    }
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

impl WorkspaceHandle {
    pub async fn list(&self) -> SubsystemResult<Vec<WorkspaceSummary>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(WorkspaceMsg::List { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?
    }

    pub async fn contains(&self, workspace_uuid: impl Into<String>) -> SubsystemResult<bool> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(WorkspaceMsg::Contains {
                workspace_uuid: workspace_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?
    }

    pub async fn get(&self, workspace_uuid: impl Into<String>) -> SubsystemResult<Workspace> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(WorkspaceMsg::Get {
                workspace_uuid: workspace_uuid.into(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?
    }
}
