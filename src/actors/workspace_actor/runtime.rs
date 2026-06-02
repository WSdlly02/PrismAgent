use crate::actors::workspace_actor::model::{
    AcquireLeaseRequest, Lease, ReleaseLeaseRequest, WORKSPACE_ACTOR, WorkspaceActor,
    WorkspaceHandle, WorkspaceMsg, WorkspaceState, WorkspaceSummary,
};
use crate::error::{SubsystemError, SubsystemResult};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;
use uuid::Uuid;

const LEASE_SECONDS: i64 = 15;

impl WorkspaceActor {
    pub fn mock(
        rx: mpsc::Receiver<WorkspaceMsg>,
        workspace_uuid: String,
        workspace_path: PathBuf,
        agents: Vec<crate::actors::workspace_actor::model::AgentSummary>,
    ) -> Self {
        let mut workspaces = HashMap::new();
        workspaces.insert(
            workspace_uuid.clone(),
            WorkspaceState {
                uuid: workspace_uuid,
                path: workspace_path,
                agents,
                lease: None,
            },
        );
        Self { rx, workspaces }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                WorkspaceMsg::List { reply } => {
                    let _ = reply.send(Ok(self.list()));
                }
                WorkspaceMsg::AcquireLease { request, reply } => {
                    let _ = reply.send(self.acquire_lease(request));
                }
                WorkspaceMsg::ReleaseLease { request, reply } => {
                    let _ = reply.send(self.release_lease(request));
                }
            }
        }
    }

    fn list(&mut self) -> Vec<WorkspaceSummary> {
        let now = chrono::Utc::now().timestamp();
        let mut workspaces = self
            .workspaces
            .values_mut()
            .map(|workspace| {
                expire_lease(workspace, now);
                WorkspaceSummary {
                    workspace_uuid: workspace.uuid.clone(),
                    workspace_path: workspace.path.clone(),
                    locked_by: workspace
                        .lease
                        .as_ref()
                        .map(|lease| lease.client_id.clone()),
                    agents: workspace.agents.clone(),
                }
            })
            .collect::<Vec<_>>();
        workspaces.sort_by(|left, right| left.workspace_path.cmp(&right.workspace_path));
        workspaces
    }

    fn acquire_lease(&mut self, request: AcquireLeaseRequest) -> SubsystemResult<Lease> {
        if request.client_id.trim().is_empty() {
            return Err(SubsystemError::invalid_input("client_id must not be empty"));
        }
        let workspace = self
            .workspaces
            .get_mut(&request.workspace_uuid)
            .ok_or_else(|| SubsystemError::not_found("workspace", &request.workspace_uuid))?;
        let now = chrono::Utc::now().timestamp();
        expire_lease(workspace, now);

        if let Some(lease) = &mut workspace.lease {
            let may_renew = lease.client_id == request.client_id
                && request.lease_token.as_deref() == Some(lease.lease_token.as_str());
            if !may_renew {
                return Err(SubsystemError::Conflict {
                    resource: "workspace_lease",
                    id: request.workspace_uuid,
                });
            }
            lease.expires_at = now + LEASE_SECONDS;
            return Ok(lease.clone());
        }

        let lease = Lease {
            lease_token: Uuid::now_v7().to_string(),
            workspace_uuid: request.workspace_uuid,
            client_id: request.client_id,
            expires_at: now + LEASE_SECONDS,
        };
        workspace.lease = Some(lease.clone());
        Ok(lease)
    }

    fn release_lease(&mut self, request: ReleaseLeaseRequest) -> SubsystemResult<()> {
        let workspace = self
            .workspaces
            .get_mut(&request.workspace_uuid)
            .ok_or_else(|| SubsystemError::not_found("workspace", &request.workspace_uuid))?;
        if workspace
            .lease
            .as_ref()
            .map(|lease| lease.lease_token.as_str())
            != Some(request.lease_token.as_str())
        {
            return Err(SubsystemError::PermissionDenied {
                action: "release workspace lease",
            });
        }
        workspace.lease = None;
        Ok(())
    }
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

    pub async fn acquire_lease(&self, request: AcquireLeaseRequest) -> SubsystemResult<Lease> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(WorkspaceMsg::AcquireLease {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?
    }

    pub async fn release_lease(&self, request: ReleaseLeaseRequest) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(WorkspaceMsg::ReleaseLease {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?;
        reply_rx
            .await
            .map_err(|_| SubsystemError::actor_dead(WORKSPACE_ACTOR))?
    }
}

fn expire_lease(workspace: &mut WorkspaceState, now: i64) {
    if workspace
        .lease
        .as_ref()
        .is_some_and(|lease| lease.expires_at <= now)
    {
        workspace.lease = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::workspace_actor::model::{AgentSummary, WorkspaceHandle, WorkspaceMsg};

    #[tokio::test]
    async fn lease_requires_token_for_renewal() {
        let (tx, rx) = mpsc::channel::<WorkspaceMsg>(8);
        WorkspaceActor::mock(
            rx,
            "workspace".to_string(),
            PathBuf::from("/tmp/workspace"),
            vec![AgentSummary {
                agent_uuid: "agent".to_string(),
                agent_name: "agent-0".to_string(),
            }],
        )
        .spawn();
        let handle = WorkspaceHandle { tx };

        let lease = handle
            .acquire_lease(AcquireLeaseRequest {
                workspace_uuid: "workspace".to_string(),
                client_id: "client".to_string(),
                lease_token: None,
            })
            .await
            .unwrap();
        assert!(
            handle
                .acquire_lease(AcquireLeaseRequest {
                    workspace_uuid: "workspace".to_string(),
                    client_id: "client".to_string(),
                    lease_token: None,
                })
                .await
                .is_err()
        );
        assert!(
            handle
                .acquire_lease(AcquireLeaseRequest {
                    workspace_uuid: "workspace".to_string(),
                    client_id: "client".to_string(),
                    lease_token: Some(lease.lease_token),
                })
                .await
                .is_ok()
        );
    }
}
