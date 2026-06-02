use crate::actors::agent_actor::model::{
    AgentEvent, AgentSnapshot, ApproveRequest, SendMessageRequest,
};
use crate::actors::shell_actor::model::{SHELL_ACTOR, ShellActor, ShellHandle, ShellMsg};
use crate::actors::workspace_actor::model::{
    AcquireLeaseRequest, Lease, ReleaseLeaseRequest, WorkspaceSummary,
};
use crate::error::{SubsystemError, SubsystemResult};
use tokio::sync::{broadcast, mpsc};

impl ShellActor {
    pub fn load(
        rx: mpsc::Receiver<ShellMsg>,
        workspace: crate::actors::workspace_actor::model::WorkspaceHandle,
        agent: crate::actors::agent_actor::model::AgentHandle,
    ) -> Self {
        Self {
            rx,
            workspace,
            agent,
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                ShellMsg::ListWorkspaces { reply } => {
                    let _ = reply.send(self.workspace.list().await);
                }
                ShellMsg::AcquireLease { request, reply } => {
                    let _ = reply.send(self.workspace.acquire_lease(request).await);
                }
                ShellMsg::ReleaseLease { request, reply } => {
                    let _ = reply.send(self.workspace.release_lease(request).await);
                }
                ShellMsg::AgentSnapshot { agent_uuid, reply } => {
                    let _ = reply.send(self.agent.snapshot(agent_uuid).await);
                }
                ShellMsg::SubscribeAgent { agent_uuid, reply } => {
                    let _ = reply.send(self.agent.subscribe(agent_uuid).await);
                }
                ShellMsg::SendMessage { request, reply } => {
                    let _ = reply.send(self.agent.send_message(request).await);
                }
                ShellMsg::ApproveRequest { request, reply } => {
                    let _ = reply.send(self.agent.approve_request(request).await);
                }
                ShellMsg::Cancel { agent_uuid, reply } => {
                    let _ = reply.send(self.agent.cancel(agent_uuid).await);
                }
            }
        }
    }
}

impl ShellHandle {
    pub async fn list_workspaces(&self) -> SubsystemResult<Vec<WorkspaceSummary>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::ListWorkspaces { reply: reply_tx })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }

    pub async fn acquire_lease(&self, request: AcquireLeaseRequest) -> SubsystemResult<Lease> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::AcquireLease {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }

    pub async fn release_lease(&self, request: ReleaseLeaseRequest) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::ReleaseLease {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }

    pub async fn agent_snapshot(&self, agent_uuid: String) -> SubsystemResult<AgentSnapshot> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::AgentSnapshot {
                agent_uuid,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }

    pub async fn subscribe_agent(
        &self,
        agent_uuid: String,
    ) -> SubsystemResult<broadcast::Receiver<AgentEvent>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::SubscribeAgent {
                agent_uuid,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }

    pub async fn send_message(&self, request: SendMessageRequest) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::SendMessage {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }

    pub async fn approve_request(&self, request: ApproveRequest) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::ApproveRequest {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }

    pub async fn cancel(&self, agent_uuid: String) -> SubsystemResult<()> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        self.tx
            .send(ShellMsg::Cancel {
                agent_uuid,
                reply: reply_tx,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?;
        recv(reply_rx).await
    }
}

async fn recv<T>(
    reply_rx: tokio::sync::oneshot::Receiver<SubsystemResult<T>>,
) -> SubsystemResult<T> {
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(SHELL_ACTOR))?
}
