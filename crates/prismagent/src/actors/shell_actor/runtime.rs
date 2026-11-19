use crate::actor_dispatch_mixed;
use crate::actors::agent_actor::model::{
    AgentSnapshot, AgentStatus, AgentSummary, ApproveRequest, SendMessageRequest,
};
use crate::actors::shell_actor::model::{
    AgentAccessRequest, AgentWriteAccessRequest, AuthorizedAgentCreateRequest,
    AuthorizedApproveRequest, AuthorizedCancelWorkflowRequest, AuthorizedDeleteWorkspaceRequest,
    AuthorizedSendMessageRequest, ConnectionId, ConnectionSession, EventTarget, SHELL_ACTOR,
    ShellActor, ShellHandle, ShellMsg, WorkspaceAccessRequest, WorkspaceWriteAccessRequest,
    WsEvent,
};
use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest};
use crate::actors::workflow_actor::model::{WorkflowCancelRequest, WorkflowCancelResponse};
use crate::actors::workspace_actor::model::{AcquireLeaseRequest, Lease, ReleaseLeaseRequest};
use crate::actors::workspace_actor::model::{WorkspaceCreateRequest, WorkspaceSummary};
use crate::error::{ConflictKind, ResourceKind, SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::id::petname_uuid;
use crate::impl_handle_methods;
use std::collections::HashSet;
use tokio::sync::mpsc;
use uuid::Uuid;

const LEASE_SECONDS: i64 = 10;
const SUBSCRIBER_BUFFER: usize = 64;

impl ShellActor {
    pub fn load(rx: mpsc::Receiver<ShellMsg>, handles: AppHandles) -> Self {
        Self {
            rx,
            handles,
            shutting_down: false,
            connections: Default::default(),
            connection_channels: Default::default(),
            leases: Default::default(),
            workspace_subscribers: Default::default(),
            agent_subscribers: Default::default(),
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            actor_dispatch_mixed!(msg;
                reply {
                    ShellMsg::TryShutdown { ; reply } => self.try_shutdown().await,
                    ShellMsg::ListWorkspaces { ; reply } => self.list_workspaces().await,
                    ShellMsg::ListProfiles { ; reply } => self.handles.profile.list_profiles().await,
                    ShellMsg::CreateWorkspace { request ; reply } => self.create_workspace(request).await,
                    ShellMsg::AcquireLease { request ; reply } => self.acquire_lease(request).await,
                    ShellMsg::ReleaseLease { request ; reply } => self.release_lease(request),
                    ShellMsg::RegisterConnection { connection_id ; reply } => self.register_connection(connection_id),
                    ShellMsg::SubscribeWorkspace { connection_id, workspace_uuid ; reply } => self.subscribe_workspace(connection_id, workspace_uuid).await,
                    ShellMsg::SubscribeAgent { connection_id, agent_uuid ; reply } => self.subscribe_agent(connection_id, agent_uuid).await,
                    ShellMsg::ListAgents { request ; reply } => self.list_agents(request).await,
                    ShellMsg::CreateAgent { request ; reply } => self.create_agent(request).await,
                    ShellMsg::DeleteAgent { request ; reply } => self.delete_agent(request).await,
                    ShellMsg::AgentSnapshot { request ; reply } => self.agent_snapshot(request).await,
                    ShellMsg::SendMessage { request ; reply } => self.send_message(request).await,
                    ShellMsg::ApproveRequest { request ; reply } => self.approve_request(request).await,
                    ShellMsg::Cancel { request ; reply } => self.cancel(request).await,
                    ShellMsg::CancelWorkflow { request ; reply } => self.workflow_cancel(request).await,
                    ShellMsg::DeleteWorkspace { request ; reply } => self.delete_workspace(request).await,
                }
                fire {
                    ShellMsg::UnregisterConnection { connection_id } => self.unregister_connection(connection_id),
                    ShellMsg::UnsubscribeWorkspace { connection_id } => self.unsubscribe_workspace(connection_id),
                    ShellMsg::UnsubscribeAgent { connection_id } => self.unsubscribe_agent(connection_id),
                    ShellMsg::EmitEvent { target, event } => self.emit_event(target, event),
                }
            );
        }
    }

    // ========== Connection lifecycle ==========

    fn register_connection(
        &mut self,
        connection_id: ConnectionId,
    ) -> SubsystemResult<mpsc::Receiver<WsEvent>> {
        let (tx, rx) = mpsc::channel(SUBSCRIBER_BUFFER);
        self.connections.insert(
            connection_id,
            ConnectionSession {
                connection_id,
                subscribed_workspace: None,
                subscribed_agent: None,
            },
        );
        self.connection_channels.insert(connection_id, tx);
        Ok(rx)
    }

    fn unregister_connection(&mut self, connection_id: ConnectionId) {
        // Remove from workspace_subscribers
        if let Some(session) = self.connections.remove(&connection_id) {
            if let Some(workspace_uuid) = session.subscribed_workspace
                && let Some(subs) = self.workspace_subscribers.get_mut(&workspace_uuid)
            {
                subs.retain(|&id| id != connection_id);
                if subs.is_empty() {
                    self.workspace_subscribers.remove(&workspace_uuid);
                }
            }
            if let Some(agent_uuid) = session.subscribed_agent
                && let Some(subs) = self.agent_subscribers.get_mut(&agent_uuid)
            {
                subs.retain(|&id| id != connection_id);
                if subs.is_empty() {
                    self.agent_subscribers.remove(&agent_uuid);
                }
            }
        }
        self.connection_channels.remove(&connection_id);
    }

    // ========== Workspace subscription (multi-reader) ==========

    async fn subscribe_workspace(
        &mut self,
        connection_id: ConnectionId,
        workspace_uuid: String,
    ) -> SubsystemResult<()> {
        // Verify connection exists
        if !self.connections.contains_key(&connection_id) {
            return Err(SubsystemError::internal(
                "subscribe workspace",
                "unknown connection id",
            ));
        }
        // Verify workspace exists
        if !self.handles.workspace.contains(&workspace_uuid).await? {
            return Err(SubsystemError::not_found(
                ResourceKind::Workspace,
                workspace_uuid,
            ));
        }
        // Remove previous workspace subscription for this connection
        self.unsubscribe_workspace(connection_id);
        // Add new subscription
        self.workspace_subscribers
            .entry(workspace_uuid.clone())
            .or_default()
            .push(connection_id);
        if let Some(session) = self.connections.get_mut(&connection_id) {
            session.subscribed_workspace = Some(workspace_uuid);
        }
        Ok(())
    }

    fn unsubscribe_workspace(&mut self, connection_id: ConnectionId) {
        let workspace_uuid = self
            .connections
            .get(&connection_id)
            .and_then(|s| s.subscribed_workspace.clone());
        if let Some(workspace_uuid) = workspace_uuid {
            if let Some(subs) = self.workspace_subscribers.get_mut(&workspace_uuid) {
                subs.retain(|&id| id != connection_id);
                if subs.is_empty() {
                    self.workspace_subscribers.remove(&workspace_uuid);
                }
            }
            if let Some(session) = self.connections.get_mut(&connection_id) {
                session.subscribed_workspace = None;
            }
        }
    }

    // ========== Agent subscription (single-reader per agent) ==========

    async fn subscribe_agent(
        &mut self,
        connection_id: ConnectionId,
        agent_uuid: String,
    ) -> SubsystemResult<()> {
        // Verify connection exists
        if !self.connections.contains_key(&connection_id) {
            return Err(SubsystemError::internal(
                "subscribe agent",
                "unknown connection id",
            ));
        }
        // Verify agent exists (use workspace from the connection's subscription)
        let workspace_uuid = self
            .connections
            .get(&connection_id)
            .and_then(|s| s.subscribed_workspace.clone())
            .ok_or_else(|| {
                SubsystemError::validation("subscribe workspace before subscribing to an agent")
            })?;
        if !self
            .handles
            .agent
            .contains(&workspace_uuid, &agent_uuid)
            .await?
        {
            return Err(SubsystemError::not_found(ResourceKind::Agent, &agent_uuid));
        }
        // Auto-unsubscribe from previous agent
        self.unsubscribe_agent(connection_id);
        // Set new agent subscription
        self.agent_subscribers
            .entry(agent_uuid.clone())
            .or_default()
            .push(connection_id);
        if let Some(session) = self.connections.get_mut(&connection_id) {
            session.subscribed_agent = Some(agent_uuid);
        }
        Ok(())
    }

    fn unsubscribe_agent(&mut self, connection_id: ConnectionId) {
        let agent_uuid = self
            .connections
            .get(&connection_id)
            .and_then(|s| s.subscribed_agent.clone());
        if let Some(agent_uuid) = agent_uuid {
            if let Some(subs) = self.agent_subscribers.get_mut(&agent_uuid) {
                subs.retain(|&id| id != connection_id);
                if subs.is_empty() {
                    self.agent_subscribers.remove(&agent_uuid);
                }
            }
            if let Some(session) = self.connections.get_mut(&connection_id) {
                session.subscribed_agent = None;
            }
        }
    }

    // ========== Event emission ==========

    fn emit_event(&mut self, target: EventTarget, event: WsEvent) {
        match target {
            EventTarget::Workspace(workspace_uuid) => {
                let Some(connection_ids) = self.workspace_subscribers.get(&workspace_uuid) else {
                    return;
                };
                let mut closed = Vec::new();
                for &conn_id in connection_ids {
                    if let Some(tx) = self.connection_channels.get(&conn_id) {
                        // Intentionally best-effort: awaiting one slow WebSocket
                        // subscriber would stall ShellActor and therefore all
                        // control-plane requests routed through it.
                        match tx.try_send(event.clone()) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                // Buffer full — skip (non-fatal with buffer=64)
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                closed.push(conn_id);
                            }
                        }
                    }
                }
                for conn_id in closed {
                    self.unregister_connection(conn_id);
                }
            }
            EventTarget::Agent(agent_uuid) => {
                let Some(connection_ids) = self.agent_subscribers.get(&agent_uuid) else {
                    return;
                };
                let mut closed = Vec::new();
                for &conn_id in connection_ids {
                    if let Some(tx) = self.connection_channels.get(&conn_id) {
                        // See the workspace branch above: subscriber backpressure
                        // must remain local to that connection.
                        match tx.try_send(event.clone()) {
                            Ok(()) => {}
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                // Buffer full — skip
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                closed.push(conn_id);
                            }
                        }
                    }
                }
                for conn_id in closed {
                    self.unregister_connection(conn_id);
                }
            }
        }
    }

    // ========== REST operations ==========

    async fn try_shutdown(&mut self) -> SubsystemResult<bool> {
        // Shell is the external work-entry point. Once this latch is set, new work is
        // rejected while completion controls (approve/cancel) remain available.
        self.shutting_down = true;
        self.handles.agent.try_shutdown().await
    }

    fn ensure_accepting_work(&self) -> SubsystemResult<()> {
        if self.shutting_down {
            Err(SubsystemError::ShuttingDown {
                component: SHELL_ACTOR,
            })
        } else {
            Ok(())
        }
    }

    async fn list_workspaces(&mut self) -> SubsystemResult<Vec<WorkspaceSummary>> {
        self.prune_expired_leases();
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
        self.ensure_accepting_work()?;
        self.handles.workspace.create(request).await
    }

    async fn acquire_lease(&mut self, request: AcquireLeaseRequest) -> SubsystemResult<Lease> {
        if request.client_id.trim().is_empty() {
            return Err(SubsystemError::validation_field(
                "client_id",
                "client_id must not be empty",
            ));
        }
        if !self
            .handles
            .workspace
            .contains(&request.workspace_uuid)
            .await?
        {
            return Err(SubsystemError::not_found(
                ResourceKind::Workspace,
                request.workspace_uuid,
            ));
        }

        self.prune_expired_leases();
        let now = chrono::Utc::now().timestamp();
        if let Some(lease) = self.leases.get_mut(&request.workspace_uuid) {
            let may_renew = lease.client_id == request.client_id
                && request.lease_token.as_deref() == Some(lease.lease_token.as_str());
            if !may_renew {
                return Err(SubsystemError::conflict(
                    ConflictKind::WorkspaceLeaseHeld,
                    request.workspace_uuid,
                ));
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

    fn release_lease(&mut self, request: ReleaseLeaseRequest) -> SubsystemResult<()> {
        self.prune_expired_leases();
        let lease = self.leases.get(&request.workspace_uuid).ok_or_else(|| {
            SubsystemError::not_found(ResourceKind::WorkspaceLease, &request.workspace_uuid)
        })?;
        if lease.lease_token != request.lease_token {
            return Err(SubsystemError::PermissionDenied {
                action: "release workspace lease",
            });
        }
        self.leases.remove(&request.workspace_uuid);
        Ok(())
    }

    async fn list_agents(
        &self,
        request: WorkspaceAccessRequest,
    ) -> SubsystemResult<Vec<AgentSummary>> {
        self.authorize_workspace_read(&request).await?;
        self.handles.agent.list(request.workspace_uuid).await
    }

    async fn create_agent(&self, request: AuthorizedAgentCreateRequest) -> SubsystemResult<Agent> {
        self.ensure_accepting_work()?;
        self.authorize_workspace_write(&request.workspace)?;
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

    async fn delete_agent(&self, request: AgentWriteAccessRequest) -> SubsystemResult<()> {
        self.ensure_accepting_work()?;
        self.authorize_agent_write(&request).await?;
        self.handles
            .agent
            .delete(request.workspace.workspace_uuid, request.agent_uuid)
            .await
    }

    async fn agent_snapshot(&self, request: AgentAccessRequest) -> SubsystemResult<AgentSnapshot> {
        self.authorize_agent_read(&request).await?;
        self.handles.agent.snapshot(request.agent_uuid).await
    }

    async fn send_message(&self, request: AuthorizedSendMessageRequest) -> SubsystemResult<()> {
        self.ensure_accepting_work()?;
        self.authorize_agent_write(&request.access).await?;
        self.handles
            .agent
            .send_message(SendMessageRequest {
                agent_uuid: request.access.agent_uuid,
                message_body: request.message_body,
            })
            .await
    }

    async fn approve_request(&self, request: AuthorizedApproveRequest) -> SubsystemResult<()> {
        self.authorize_agent_write(&request.access).await?;
        self.handles
            .agent
            .approve_request(ApproveRequest {
                agent_uuid: request.access.agent_uuid,
                request_uuid: request.request_uuid,
                approval_mask: request.approval_mask,
            })
            .await
    }

    async fn cancel(&self, request: AgentWriteAccessRequest) -> SubsystemResult<()> {
        self.authorize_agent_write(&request).await?;
        self.handles.agent.cancel(request.agent_uuid).await
    }

    async fn workflow_cancel(
        &self,
        request: AuthorizedCancelWorkflowRequest,
    ) -> SubsystemResult<WorkflowCancelResponse> {
        self.authorize_workspace_write(&request.workspace)?;
        self.handles
            .workflow
            .workflow_cancel(WorkflowCancelRequest {
                workspace_uuid: request.workspace.workspace_uuid,
                workflow_uuid: request.workflow_uuid,
                reason: request.reason,
            })
            .await
    }

    async fn delete_workspace(
        &mut self,
        request: AuthorizedDeleteWorkspaceRequest,
    ) -> SubsystemResult<()> {
        self.ensure_accepting_work()?;
        // 1. Verify lease token
        self.authorize_workspace_write(&WorkspaceWriteAccessRequest {
            workspace_uuid: request.workspace_uuid.clone(),
            lease_token: request.lease_token.clone(),
        })?;

        // 2. List all agents in this workspace
        let agents = self
            .handles
            .agent
            .list(request.workspace_uuid.clone())
            .await?;

        // 3. Check each agent's status — block if any not Idle
        for agent in &agents {
            if agent.status != AgentStatus::Idle {
                return Err(SubsystemError::conflict(
                    ConflictKind::AgentBusy,
                    format!(
                        "{} (status: {:?}, agent: {})",
                        agent.agent_uuid, agent.status, agent.agent_name
                    ),
                ));
            }
        }

        // 4. Invalidate the reconstructible AgentActor cache before the durable
        // commit point. If this fails, the workspace remains untouched. If the
        // following delete fails, a later AgentActor request reloads from storage.
        self.handles
            .agent
            .forget_workspace(request.workspace_uuid.clone())
            .await?;

        // 5. Commit point: atomically move the workspace directory out of the
        // active tree. Nothing after this step is allowed to fail.
        self.handles
            .workspace
            .delete(request.workspace_uuid.clone())
            .await?;

        // 6. Notify subscribers before removing their routing entries.
        self.emit_event(
            EventTarget::Workspace(request.workspace_uuid.clone()),
            WsEvent::WorkspaceDeleted {
                workspace_uuid: request.workspace_uuid.clone(),
            },
        );

        // 7. All remaining cleanup is infallible in-memory state removal.
        self.clear_deleted_workspace_state(&request.workspace_uuid, &agents);

        Ok(())
    }

    fn clear_deleted_workspace_state(&mut self, workspace_uuid: &str, agents: &[AgentSummary]) {
        let mut deleted_agent_uuids = agents
            .iter()
            .map(|agent| agent.agent_uuid.clone())
            .collect::<HashSet<_>>();

        // Include stale per-connection agent references associated with this
        // workspace, even if they were absent from AgentActor's latest list.
        for session in self.connections.values() {
            if session.subscribed_workspace.as_deref() == Some(workspace_uuid)
                && let Some(agent_uuid) = &session.subscribed_agent
            {
                deleted_agent_uuids.insert(agent_uuid.clone());
            }
        }

        self.leases.remove(workspace_uuid);
        self.workspace_subscribers.remove(workspace_uuid);
        self.agent_subscribers
            .retain(|agent_uuid, _| !deleted_agent_uuids.contains(agent_uuid));

        for session in self.connections.values_mut() {
            if session.subscribed_workspace.as_deref() == Some(workspace_uuid) {
                session.subscribed_workspace = None;
            }
            if session
                .subscribed_agent
                .as_ref()
                .is_some_and(|agent_uuid| deleted_agent_uuids.contains(agent_uuid))
            {
                session.subscribed_agent = None;
            }
        }
    }

    // ========== Authorization ==========

    async fn authorize_agent_read(&self, request: &AgentAccessRequest) -> SubsystemResult<()> {
        self.authorize_workspace_read(&request.workspace).await?;
        if !self
            .handles
            .agent
            .contains(&request.workspace.workspace_uuid, &request.agent_uuid)
            .await?
        {
            return Err(SubsystemError::not_found(
                ResourceKind::Agent,
                &request.agent_uuid,
            ));
        }
        Ok(())
    }

    async fn authorize_agent_write(
        &self,
        request: &AgentWriteAccessRequest,
    ) -> SubsystemResult<()> {
        self.authorize_workspace_write(&request.workspace)?;
        if !self
            .handles
            .agent
            .contains(&request.workspace.workspace_uuid, &request.agent_uuid)
            .await?
        {
            return Err(SubsystemError::not_found(
                ResourceKind::Agent,
                &request.agent_uuid,
            ));
        }
        Ok(())
    }

    async fn authorize_workspace_read(
        &self,
        request: &WorkspaceAccessRequest,
    ) -> SubsystemResult<()> {
        if !self
            .handles
            .workspace
            .contains(&request.workspace_uuid)
            .await?
        {
            return Err(SubsystemError::not_found(
                ResourceKind::Workspace,
                &request.workspace_uuid,
            ));
        }
        Ok(())
    }

    fn authorize_workspace_write(
        &self,
        request: &WorkspaceWriteAccessRequest,
    ) -> SubsystemResult<()> {
        let lease = self
            .leases
            .get(&request.workspace_uuid)
            .filter(|lease| lease.expires_at > chrono::Utc::now().timestamp())
            .ok_or(SubsystemError::PermissionDenied {
                action: "write workspace without active lease",
            })?;
        if lease.lease_token != request.lease_token {
            return Err(SubsystemError::PermissionDenied {
                action: "write workspace with invalid lease",
            });
        }
        Ok(())
    }

    fn prune_expired_leases(&mut self) {
        let now = chrono::Utc::now().timestamp();
        self.leases.retain(|_, lease| lease.expires_at > now);
    }
}

// ========== ShellHandle convenience methods (macro-generated) ==========

impl_handle_methods! {
    ShellHandle for ShellMsg, SHELL_ACTOR;

    fn try_shutdown(&self) -> bool
        => TryShutdown {};

    fn list_workspaces(&self) -> Vec<WorkspaceSummary>
        => ListWorkspaces {};

    fn create_workspace(&self, request: WorkspaceCreateRequest) -> WorkspaceSummary
        => CreateWorkspace { request: request };

    fn acquire_lease(&self, request: AcquireLeaseRequest) -> Lease
        => AcquireLease { request: request };

    fn release_lease(&self, request: ReleaseLeaseRequest) -> ()
        => ReleaseLease { request: request };

    fn register_connection(&self, connection_id: ConnectionId) -> mpsc::Receiver<WsEvent>
        => RegisterConnection { connection_id: connection_id };

    fn subscribe_workspace(&self, connection_id: ConnectionId, workspace_uuid: impl Into<String>) -> ()
        => SubscribeWorkspace { connection_id: connection_id, workspace_uuid: workspace_uuid.into() };

    fn subscribe_agent(&self, connection_id: ConnectionId, agent_uuid: impl Into<String>) -> ()
        => SubscribeAgent { connection_id: connection_id, agent_uuid: agent_uuid.into() };

    fn list_agents(&self, request: WorkspaceAccessRequest) -> Vec<AgentSummary>
        => ListAgents { request: request };

    fn create_agent(&self, request: AuthorizedAgentCreateRequest) -> Agent
        => CreateAgent { request: request };

    fn delete_agent(&self, request: AgentWriteAccessRequest) -> ()
        => DeleteAgent { request: request };

    fn list_profiles(&self) -> Vec<String>
        => ListProfiles {};

    fn agent_snapshot(&self, request: AgentAccessRequest) -> AgentSnapshot
        => AgentSnapshot { request: request };

    fn send_message(&self, request: AuthorizedSendMessageRequest) -> ()
        => SendMessage { request: request };

    fn approve_request(&self, request: AuthorizedApproveRequest) -> ()
        => ApproveRequest { request: request };

    fn cancel(&self, request: AgentWriteAccessRequest) -> ()
        => Cancel { request: request };

    fn workflow_cancel(&self, request: AuthorizedCancelWorkflowRequest) -> WorkflowCancelResponse
        => CancelWorkflow { request: request };

    fn delete_workspace(&self, request: AuthorizedDeleteWorkspaceRequest) -> ()
        => DeleteWorkspace { request: request };
}

// ========== ShellHandle fire-and-forget methods ==========

impl ShellHandle {
    pub fn unregister_connection(&self, connection_id: ConnectionId) {
        let _ = self
            .tx
            .try_send(ShellMsg::UnregisterConnection { connection_id });
    }

    // ---- Subscription ----

    pub fn unsubscribe_workspace(&self, connection_id: ConnectionId) {
        let _ = self
            .tx
            .try_send(ShellMsg::UnsubscribeWorkspace { connection_id });
    }

    pub fn unsubscribe_agent(&self, connection_id: ConnectionId) {
        let _ = self
            .tx
            .try_send(ShellMsg::UnsubscribeAgent { connection_id });
    }

    // ---- Event emission (convenience) ----

    pub fn emit_agent_event(
        &self,
        agent_uuid: impl Into<String>,
        event: WsEvent,
    ) -> SubsystemResult<()> {
        // Deliberately use try_send: callers are actors that ShellActor may be
        // awaiting, so send().await on a full mailbox could create a wait cycle.
        self.tx
            .try_send(ShellMsg::EmitEvent {
                target: EventTarget::Agent(agent_uuid.into()),
                event,
            })
            .map_err(|error| SubsystemError::internal("enqueue agent event", error.to_string()))
    }

    pub fn emit_workspace_event(
        &self,
        workspace_uuid: impl Into<String>,
        event: WsEvent,
    ) -> SubsystemResult<()> {
        // Keep all cross-actor event emission non-blocking; see
        // emit_agent_event for the wait-cycle rationale.
        self.tx
            .try_send(ShellMsg::EmitEvent {
                target: EventTarget::Workspace(workspace_uuid.into()),
                event,
            })
            .map_err(|error| SubsystemError::internal("enqueue workspace event", error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::agent_actor::model::{AgentHandle, AgentMsg};
    use crate::actors::workspace_actor::model::{WorkspaceActor, WorkspaceHandle, WorkspaceMsg};
    use std::path::PathBuf;
    use uuid::Uuid;

    async fn workspace_fixture() -> (PathBuf, WorkspaceHandle) {
        let root = std::env::temp_dir().join(format!("prismagent-test-{}", Uuid::now_v7()));
        let workspace_root = root.join("workspace");
        std::fs::create_dir_all(&workspace_root).unwrap();
        std::fs::write(
            workspace_root.join("metadata.json"),
            r#"{"uuid":"workspace","path":"/tmp/workspace"}"#,
        )
        .unwrap();

        let (tx, rx) = mpsc::channel::<WorkspaceMsg>(8);
        let handle = WorkspaceHandle { tx };
        WorkspaceActor::from_root(rx, root.clone()).unwrap().spawn();
        handle.list().await.unwrap();
        (root, handle)
    }

    fn mock_agent_handle(
        agents: Vec<AgentSummary>,
        forget_result: SubsystemResult<()>,
    ) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<AgentMsg>(8);
        tokio::spawn(async move {
            let mut agents = Some(agents);
            let mut forget_result = Some(forget_result);
            while let Some(message) = rx.recv().await {
                match message {
                    AgentMsg::List { reply, .. } => {
                        let _ = reply.send(Ok(agents.take().unwrap_or_default()));
                    }
                    AgentMsg::ForgetWorkspace { reply, .. } => {
                        let _ = reply.send(forget_result.take().unwrap_or(Ok(())));
                    }
                    _ => panic!("unexpected AgentMsg in workspace deletion test"),
                }
            }
        });
        AgentHandle { tx }
    }

    fn idle_agent(agent_uuid: &str) -> AgentSummary {
        AgentSummary {
            agent_uuid: agent_uuid.to_string(),
            agent_name: agent_uuid.to_string(),
            profile: "default".to_string(),
            auto_loop: false,
            context_refs: Vec::new(),
            context_out: Vec::new(),
            status: AgentStatus::Idle,
        }
    }

    #[tokio::test]
    async fn try_shutdown_latches_shell_and_rejects_new_work() {
        let (agent_tx, mut agent_rx) = mpsc::channel::<AgentMsg>(1);
        tokio::spawn(async move {
            match agent_rx.recv().await {
                Some(AgentMsg::TryShutdown { reply }) => {
                    let _ = reply.send(Ok(false));
                }
                _ => panic!("expected AgentMsg::TryShutdown"),
            }
        });

        let mut handles = crate::handles::test_handles();
        handles.agent = AgentHandle { tx: agent_tx };
        let (_shell_tx, shell_rx) = mpsc::channel(1);
        let mut shell = ShellActor::load(shell_rx, handles);

        assert!(!shell.try_shutdown().await.unwrap());
        assert!(shell.shutting_down);

        let error = shell
            .create_workspace(WorkspaceCreateRequest {
                path: PathBuf::from("/tmp/new-work"),
            })
            .await
            .unwrap_err();
        assert!(matches!(error, SubsystemError::ShuttingDown { .. }));
    }

    #[tokio::test]
    async fn multiple_connections_can_subscribe_same_workspace() {
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

        // Register two connections
        let _rx1 = shell.register_connection(1).unwrap();
        let _rx2 = shell.register_connection(2).unwrap();

        // Both can subscribe to the same workspace
        assert!(
            shell
                .subscribe_workspace(1, "workspace".to_string())
                .await
                .is_ok()
        );
        assert!(
            shell
                .subscribe_workspace(2, "workspace".to_string())
                .await
                .is_ok()
        );

        // Workspace has two subscribers
        assert_eq!(
            shell.workspace_subscribers.get("workspace").unwrap().len(),
            2
        );
    }

    #[tokio::test]
    async fn lease_requires_matching_token_for_workspace_writes() {
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
                .authorize_workspace_read(&WorkspaceAccessRequest {
                    workspace_uuid: "workspace".to_string(),
                })
                .await
                .is_ok()
        );
        assert!(
            shell
                .authorize_workspace_write(&WorkspaceWriteAccessRequest {
                    workspace_uuid: "workspace".to_string(),
                    lease_token: lease.lease_token,
                })
                .is_ok()
        );
        assert!(
            shell
                .authorize_workspace_write(&WorkspaceWriteAccessRequest {
                    workspace_uuid: "workspace".to_string(),
                    lease_token: "wrong-token".to_string(),
                })
                .is_err()
        );
    }

    #[tokio::test]
    async fn unregister_connection_cleans_up_subscriptions() {
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

        shell.register_connection(1).unwrap();
        shell
            .subscribe_workspace(1, "workspace".to_string())
            .await
            .unwrap();

        assert_eq!(
            shell.workspace_subscribers.get("workspace").unwrap().len(),
            1
        );

        shell.unregister_connection(1);

        assert!(!shell.workspace_subscribers.contains_key("workspace"));
    }

    #[tokio::test]
    async fn workspace_delete_does_not_commit_when_agent_cache_invalidation_fails() {
        let (root, workspace) = workspace_fixture().await;
        let mut handles = crate::handles::test_handles();
        handles.workspace = workspace;
        handles.agent = mock_agent_handle(Vec::new(), Err(SubsystemError::actor_dead("agent")));
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

        let result = shell
            .delete_workspace(AuthorizedDeleteWorkspaceRequest {
                workspace_uuid: "workspace".to_string(),
                lease_token: lease.lease_token,
            })
            .await;

        assert!(matches!(result, Err(SubsystemError::ActorDead { .. })));
        assert!(root.join("workspace").exists());
        assert!(shell.handles.workspace.contains("workspace").await.unwrap());
        assert!(shell.leases.contains_key("workspace"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn successful_workspace_delete_clears_all_shell_subscription_state() {
        let (root, workspace) = workspace_fixture().await;
        let mut handles = crate::handles::test_handles();
        handles.workspace = workspace;
        handles.agent = mock_agent_handle(vec![idle_agent("agent-1")], Ok(()));
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

        let mut events = shell.register_connection(1).unwrap();
        let session = shell.connections.get_mut(&1).unwrap();
        session.subscribed_workspace = Some("workspace".to_string());
        session.subscribed_agent = Some("stale-agent".to_string());
        shell
            .workspace_subscribers
            .insert("workspace".to_string(), vec![1]);
        shell
            .agent_subscribers
            .insert("agent-1".to_string(), vec![99]);
        shell
            .agent_subscribers
            .insert("stale-agent".to_string(), vec![1]);

        shell
            .delete_workspace(AuthorizedDeleteWorkspaceRequest {
                workspace_uuid: "workspace".to_string(),
                lease_token: lease.lease_token,
            })
            .await
            .unwrap();

        assert!(!root.join("workspace").exists());
        assert!(!shell.leases.contains_key("workspace"));
        assert!(!shell.workspace_subscribers.contains_key("workspace"));
        assert!(!shell.agent_subscribers.contains_key("agent-1"));
        assert!(!shell.agent_subscribers.contains_key("stale-agent"));
        let session = shell.connections.get(&1).unwrap();
        assert_eq!(session.subscribed_workspace, None);
        assert_eq!(session.subscribed_agent, None);
        assert!(matches!(
            events.try_recv().unwrap(),
            WsEvent::WorkspaceDeleted { workspace_uuid } if workspace_uuid == "workspace"
        ));
        std::fs::remove_dir_all(root).unwrap();
    }
}
