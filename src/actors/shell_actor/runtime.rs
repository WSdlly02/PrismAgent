use crate::actors::agent_actor::model::{
    AgentEvent, AgentSnapshot, AgentSummary, ApproveRequest, SendMessageRequest,
};
use crate::actors::shell_actor::model::{
    AgentAccessRequest, AuthorizedAgentCreateRequest, AuthorizedApproveRequest,
    AuthorizedSendMessageRequest, AuthorizedWorkflowCancelRequest, SHELL_ACTOR, ShellActor,
    ShellHandle, ShellMsg, WorkspaceAccessRequest,
};
use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest};
use crate::actors::workflow_actor::model::{WorkflowCancelRequest, WorkflowCancelResponse};
use crate::actors::workspace_actor::model::{
    AcquireLeaseRequest, Lease, ReleaseLeaseRequest, WorkspaceCreateRequest, WorkspaceSummary,
};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::id::petname_uuid;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

const LEASE_SECONDS: i64 = 15;
const SUBSCRIBER_BUFFER: usize = 128;

impl ShellActor {
    pub fn load(rx: mpsc::Receiver<ShellMsg>, handles: AppHandles) -> Self {
        Self {
            rx,
            handles,
            leases: Default::default(),
            subscribers: Default::default(),
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ShellMsg::ListWorkspaces { reply } => {
                    let _ = reply.send(self.list_workspaces().await);
                }
                ShellMsg::ListProfiles { reply } => {
                    let _ = reply.send(self.handles.profile.list_profiles().await);
                }
                ShellMsg::CreateWorkspace { request, reply } => {
                    let _ = reply.send(self.create_workspace(request).await);
                }
                ShellMsg::AcquireLease { request, reply } => {
                    let _ = reply.send(self.acquire_lease(request).await);
                }
                ShellMsg::ReleaseLease { request, reply } => {
                    let _ = reply.send(self.release_lease(request));
                }
                ShellMsg::ListAgents { request, reply } => {
                    let _ = reply.send(self.list_agents(request).await);
                }
                ShellMsg::CreateAgent { request, reply } => {
                    let _ = reply.send(self.create_agent(request).await);
                }
                ShellMsg::AgentSnapshot { request, reply } => {
                    let _ = reply.send(self.agent_snapshot(request).await);
                }
                ShellMsg::SubscribeAgent { request, reply } => {
                    let _ = reply.send(self.subscribe_agent(request).await);
                }
                ShellMsg::SendMessage { request, reply } => {
                    let _ = reply.send(self.send_message(request).await);
                }
                ShellMsg::ApproveRequest { request, reply } => {
                    let _ = reply.send(self.approve_request(request).await);
                }
                ShellMsg::Cancel { request, reply } => {
                    let _ = reply.send(self.cancel(request).await);
                }
                ShellMsg::WorkflowCancel { request, reply } => {
                    let _ = reply.send(self.workflow_cancel(request).await);
                }
                ShellMsg::EmitAgentEvent { agent_uuid, event } => {
                    self.emit_agent_event(&agent_uuid, event);
                }
            }
        }
    }

    async fn list_workspaces(&mut self) -> SubsystemResult<Vec<WorkspaceSummary>> {
        let now = chrono::Utc::now().timestamp();
        self.leases.retain(|_, lease| lease.expires_at > now);
        let mut workspaces = self.handles.workspace.list().await?;
        for workspace in &mut workspaces {
            workspace.locked_by = self
                .leases
                .get(&workspace.workspace_uuid)
                .map(|lease| lease.client_id.clone());
        }
        Ok(workspaces)
    }

    async fn create_workspace(
        &mut self,
        request: WorkspaceCreateRequest,
    ) -> SubsystemResult<WorkspaceSummary> {
        self.handles.workspace.create(request).await
    }

    async fn acquire_lease(&mut self, request: AcquireLeaseRequest) -> SubsystemResult<Lease> {
        if request.client_id.trim().is_empty() {
            return Err(SubsystemError::invalid_input("client_id must not be empty"));
        }
        if !self
            .handles
            .workspace
            .contains(&request.workspace_uuid)
            .await?
        {
            return Err(SubsystemError::not_found(
                "workspace",
                request.workspace_uuid,
            ));
        }
        let now = chrono::Utc::now().timestamp();
        if let Some(lease) = self.leases.get_mut(&request.workspace_uuid)
            && lease.expires_at > now
        {
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
            workspace_uuid: request.workspace_uuid.clone(),
            client_id: request.client_id,
            expires_at: now + LEASE_SECONDS,
        };
        self.leases.insert(request.workspace_uuid, lease.clone());
        Ok(lease)
    }

    async fn list_agents(
        &self,
        request: WorkspaceAccessRequest,
    ) -> SubsystemResult<Vec<AgentSummary>> {
        self.authorize_workspace(&request)?;
        self.handles.agent.list(request.workspace_uuid).await
    }

    async fn create_agent(&self, request: AuthorizedAgentCreateRequest) -> SubsystemResult<Agent> {
        self.authorize_workspace(&request.workspace)?;
        let existing = self
            .handles
            .agent
            .list(request.workspace.workspace_uuid.clone())
            .await?
            .into_iter()
            .map(|agent| agent.agent_uuid)
            .collect::<Vec<_>>();
        self.handles
            .agent
            .create(AgentCreateRequest {
                workspace_uuid: request.workspace.workspace_uuid,
                uuid: petname_uuid(existing)?,
                name: request.agent.name,
                profile: request.agent.profile,
                context_refs: request.agent.context_refs,
                context_out: request.agent.context_out,
            })
            .await
    }

    fn release_lease(&mut self, request: ReleaseLeaseRequest) -> SubsystemResult<()> {
        let lease = self
            .leases
            .get(&request.workspace_uuid)
            .ok_or_else(|| SubsystemError::not_found("workspace_lease", &request.workspace_uuid))?;
        if lease.lease_token != request.lease_token {
            return Err(SubsystemError::PermissionDenied {
                action: "release workspace lease",
            });
        }
        self.leases.remove(&request.workspace_uuid);
        Ok(())
    }

    async fn agent_snapshot(&self, request: AgentAccessRequest) -> SubsystemResult<AgentSnapshot> {
        self.authorize_agent(&request).await?;
        self.handles.agent.snapshot(request.agent_uuid).await
    }

    async fn subscribe_agent(
        &mut self,
        request: AgentAccessRequest,
    ) -> SubsystemResult<mpsc::Receiver<AgentEvent>> {
        self.authorize_agent(&request).await?;
        let (tx, rx) = mpsc::channel(SUBSCRIBER_BUFFER);
        self.subscribers.insert(request.agent_uuid, tx);
        Ok(rx)
    }

    async fn send_message(&self, request: AuthorizedSendMessageRequest) -> SubsystemResult<()> {
        self.authorize_agent(&request.access).await?;
        self.handles
            .agent
            .send_message(SendMessageRequest {
                agent_uuid: request.access.agent_uuid,
                message_body: request.message_body,
            })
            .await
    }

    async fn approve_request(&self, request: AuthorizedApproveRequest) -> SubsystemResult<()> {
        self.authorize_agent(&request.access).await?;
        self.handles
            .agent
            .approve_request(ApproveRequest {
                agent_uuid: request.access.agent_uuid,
                request_uuid: request.request_uuid,
                approval_mask: request.approval_mask,
            })
            .await
    }

    async fn cancel(&self, request: AgentAccessRequest) -> SubsystemResult<()> {
        self.authorize_agent(&request).await?;
        self.handles.agent.cancel(request.agent_uuid).await
    }

    async fn workflow_cancel(
        &self,
        request: AuthorizedWorkflowCancelRequest,
    ) -> SubsystemResult<WorkflowCancelResponse> {
        self.authorize_workspace(&request.workspace)?;
        self.handles
            .workflow
            .workflow_cancel(WorkflowCancelRequest {
                workspace_uuid: request.workspace.workspace_uuid,
                workflow_uuid: request.workflow_uuid,
                reason: request.reason,
            })
            .await
    }

    async fn authorize_agent(&self, request: &AgentAccessRequest) -> SubsystemResult<()> {
        self.authorize_workspace(&request.workspace)?;
        if !self
            .handles
            .agent
            .contains(&request.workspace.workspace_uuid, &request.agent_uuid)
            .await?
        {
            return Err(SubsystemError::not_found("agent", &request.agent_uuid));
        }
        Ok(())
    }

    fn authorize_workspace(&self, request: &WorkspaceAccessRequest) -> SubsystemResult<()> {
        let lease = self
            .leases
            .get(&request.workspace_uuid)
            .filter(|lease| lease.expires_at > chrono::Utc::now().timestamp())
            .ok_or(SubsystemError::PermissionDenied {
                action: "access workspace without active lease",
            })?; // no heap-allocating so we use ok_or instead of ok_or_else
        if lease.client_id != request.client_id || lease.lease_token != request.lease_token {
            return Err(SubsystemError::PermissionDenied {
                action: "access workspace with invalid lease",
            });
        }
        Ok(())
    }

    fn emit_agent_event(&mut self, agent_uuid: &str, event: AgentEvent) {
        let Some(subscriber) = self.subscribers.get(agent_uuid) else {
            return;
        };
        if subscriber.try_send(event).is_err() {
            self.subscribers.remove(agent_uuid);
        }
    }
}

impl ShellHandle {
    pub async fn list_workspaces(&self) -> SubsystemResult<Vec<WorkspaceSummary>> {
        request(&self.tx, |reply| ShellMsg::ListWorkspaces { reply }).await
    }

    pub async fn create_workspace(
        &self,
        request_body: WorkspaceCreateRequest,
    ) -> SubsystemResult<WorkspaceSummary> {
        request(&self.tx, |reply| ShellMsg::CreateWorkspace {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn list_agents(
        &self,
        request_body: WorkspaceAccessRequest,
    ) -> SubsystemResult<Vec<AgentSummary>> {
        request(&self.tx, |reply| ShellMsg::ListAgents {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn create_agent(
        &self,
        request_body: AuthorizedAgentCreateRequest,
    ) -> SubsystemResult<Agent> {
        request(&self.tx, |reply| ShellMsg::CreateAgent {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn list_profiles(&self) -> SubsystemResult<Vec<String>> {
        request(&self.tx, |reply| ShellMsg::ListProfiles { reply }).await
    }

    pub async fn acquire_lease(&self, request_body: AcquireLeaseRequest) -> SubsystemResult<Lease> {
        request(&self.tx, |reply| ShellMsg::AcquireLease {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn release_lease(&self, request_body: ReleaseLeaseRequest) -> SubsystemResult<()> {
        request(&self.tx, |reply| ShellMsg::ReleaseLease {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn agent_snapshot(
        &self,
        request_body: AgentAccessRequest,
    ) -> SubsystemResult<AgentSnapshot> {
        request(&self.tx, |reply| ShellMsg::AgentSnapshot {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn subscribe_agent(
        &self,
        request_body: AgentAccessRequest,
    ) -> SubsystemResult<mpsc::Receiver<AgentEvent>> {
        request(&self.tx, |reply| ShellMsg::SubscribeAgent {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn send_message(
        &self,
        request_body: AuthorizedSendMessageRequest,
    ) -> SubsystemResult<()> {
        request(&self.tx, |reply| ShellMsg::SendMessage {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn approve_request(
        &self,
        request_body: AuthorizedApproveRequest,
    ) -> SubsystemResult<()> {
        request(&self.tx, |reply| ShellMsg::ApproveRequest {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn cancel(&self, request_body: AgentAccessRequest) -> SubsystemResult<()> {
        request(&self.tx, |reply| ShellMsg::Cancel {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn workflow_cancel(
        &self,
        request_body: AuthorizedWorkflowCancelRequest,
    ) -> SubsystemResult<WorkflowCancelResponse> {
        request(&self.tx, |reply| ShellMsg::WorkflowCancel {
            request: request_body,
            reply,
        })
        .await
    }

    pub fn emit_agent_event(
        &self,
        agent_uuid: impl Into<String>,
        event: AgentEvent,
    ) -> SubsystemResult<()> {
        self.tx
            .try_send(ShellMsg::EmitAgentEvent {
                agent_uuid: agent_uuid.into(),
                event,
            })
            .map_err(|error| {
                SubsystemError::internal(format!("failed to enqueue shell event: {error}"))
            })
    }
}

async fn request<T>(
    tx: &mpsc::Sender<ShellMsg>,
    message: impl FnOnce(oneshot::Sender<SubsystemResult<T>>) -> ShellMsg,
) -> SubsystemResult<T> {
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send(message(reply_tx))
        .await
        .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::workspace_actor::model::{WorkspaceActor, WorkspaceHandle, WorkspaceMsg};

    #[tokio::test]
    async fn lease_requires_matching_client_and_token_for_renewal() {
        let root = std::env::temp_dir().join(format!("prismagent-test-{}", Uuid::now_v7()));
        let workspace_root = root.join("workspace");
        std::fs::create_dir_all(&workspace_root).unwrap();
        std::fs::write(
            workspace_root.join("metadata.json"),
            r#"{"uuid":"workspace","path":"/tmp/workspace"}"#,
        )
        .unwrap();
        let (workspace_tx, workspace_rx) = mpsc::channel::<WorkspaceMsg>(8);
        let mut handles = crate::handles::test_handles();
        handles.workspace = WorkspaceHandle { tx: workspace_tx };
        WorkspaceActor::from_root(workspace_rx, root)
            .unwrap()
            .spawn();
        handles.workspace.list().await.unwrap();
        let (_shell_tx, shell_rx) = mpsc::channel(8);
        let mut shell = ShellActor::load(shell_rx, handles);
        let lease = shell
            .acquire_lease(AcquireLeaseRequest {
                workspace_uuid: "workspace".to_string(),
                client_id: "client".to_string(),
                lease_token: None,
            })
            .await
            .unwrap();

        assert!(
            shell
                .acquire_lease(AcquireLeaseRequest {
                    workspace_uuid: "workspace".to_string(),
                    client_id: "client".to_string(),
                    lease_token: None,
                })
                .await
                .is_err()
        );
        assert!(
            shell
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
