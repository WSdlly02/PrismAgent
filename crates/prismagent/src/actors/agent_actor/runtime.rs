use crate::actors::agent_actor::model::{
    AGENT_ACTOR, AgentActor, AgentHandle, AgentInferenceOutput, AgentMsg, AgentSnapshot,
    AgentSummary, AgentTaskError, AgentTaskResult, ApproveRequest, PendingApproval,
    SelfUpdateRequest, SendMessageRequest, ToolBatchOutput,
};
use crate::actors::agent_actor::state::{
    AgentEntry, AgentState, ApprovalMask, NextAction, TurnContext,
};
use crate::actors::context_actor::model::RenderInitialPromptsRequest;
use crate::actors::shell_actor::model::WsEvent;
use crate::actors::storage_actor::model::agent::{
    Agent, AgentCreateRequest, AgentUpdateRequest as StorageAgentUpdateRequest,
};
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::{ConflictKind, ResourceKind, SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::id::AgentAddr;
use crate::{actor_dispatch_mixed, impl_handle_methods};
use std::collections::HashMap;
use tokio::sync::mpsc;

impl AgentActor {
    pub fn load(rx: mpsc::Receiver<AgentMsg>, handles: AppHandles) -> Self {
        Self {
            rx,
            entries: HashMap::new(),
            handles,
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            actor_dispatch_mixed!(msg;
                reply {
                    AgentMsg::TryShutdown { ; reply } => self.try_shutdown(),
                    AgentMsg::List { workspace_uuid ; reply } => self.list(&workspace_uuid).await,
                    AgentMsg::Create { request ; reply } => self.create(request).await,
                    AgentMsg::Delete { addr ; reply } => self.delete(addr).await,
                    AgentMsg::ForgetWorkspace { workspace_uuid ; reply } => self.forget_workspace(&workspace_uuid),
                    AgentMsg::Contains { addr ; reply } => self.contains(&addr).await,
                    AgentMsg::Snapshot { addr ; reply } => self.snapshot(&addr).await,
                    AgentMsg::SendMessage { request ; reply } => self.send_message(request).await,
                    AgentMsg::SelfUpdate { request ; reply } => self.self_update(request).await,
                    AgentMsg::ApproveRequest { request ; reply } => self.approve_request(request).await,
                    AgentMsg::Cancel { addr ; reply } => self.cancel(&addr).await,
                    AgentMsg::SetAutoLoop { addr, enabled ; reply } => self.set_auto_loop(&addr, enabled).await,
                }
                fire {
                    AgentMsg::InferenceFinished { addr, inference_uuid, result } => self.finish_inference(&addr, &inference_uuid, result).await,
                    AgentMsg::ToolBatchFinished { addr, job_uuid, result } => self.finish_tool_batch(&addr, &job_uuid, result).await,
                }
            );
        }
    }

    async fn list(&mut self, workspace_uuid: &str) -> SubsystemResult<Vec<AgentSummary>> {
        let uuids = self.handles.storage.list_agents(workspace_uuid).await?;
        let uncached = uuids
            .into_iter()
            .filter(|uuid| {
                !self
                    .entries
                    .contains_key(&AgentAddr::new(workspace_uuid, uuid))
            })
            .collect::<Vec<_>>();
        let discovered = self
            .handles
            .storage
            .read_agents(workspace_uuid, uncached)
            .await?;
        for agent in discovered {
            self.entries.insert(
                AgentAddr::new(workspace_uuid, &agent.uuid),
                AgentEntry::idle(agent),
            );
        }
        let mut agents = self
            .entries
            .iter()
            .filter(|(addr, _)| addr.workspace_uuid == workspace_uuid)
            .map(|(_, entry)| self.agent_summary(entry))
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.agent_name.cmp(&right.agent_name));
        Ok(agents)
    }

    async fn create(&mut self, request: AgentCreateRequest) -> SubsystemResult<Agent> {
        let workspace_uuid = request.addr.workspace_uuid.clone();
        let profile_name = request.profile.clone();
        let context_refs = request.context_refs.clone();
        let has_initial_task = !context_refs.is_empty();
        let profile = self.handles.profile.profile(&profile_name).await?;
        let auto_loop = profile.prompts.auto_loop;
        let auto_loop_message = profile.prompts.auto_loop_message.clone();
        let initial_units = self
            .handles
            .context
            .render_initial_prompts(Box::new(RenderInitialPromptsRequest {
                addr: request.addr.clone(),
                context_refs,
                profile,
            }))
            .await?;
        let mut agent = self
            .handles
            .storage
            .create_agent(request, auto_loop, auto_loop_message)
            .await?;
        // Storage returns the canonical ID used for the file name.
        let addr = AgentAddr::new(workspace_uuid, &agent.uuid);
        if !initial_units.is_empty() {
            agent = self
                .handles
                .storage
                .append_agent_units(addr.clone(), initial_units)
                .await?;
        }
        self.entries
            .insert(addr.clone(), AgentEntry::idle(agent.clone()));
        let summary =
            self.agent_summary(self.entries.get(&addr).expect("agent entry inserted above"));
        self.emit_workspace_event(
            &addr.workspace_uuid,
            WsEvent::AgentCreated { agent: summary },
        );
        if has_initial_task && auto_loop {
            self.apply_next_action(
                &addr,
                NextAction::ContinueInference {
                    turn: TurnContext::default(),
                },
            )?;
        }
        Ok(agent)
    }

    async fn delete(&mut self, addr: AgentAddr) -> SubsystemResult<()> {
        self.ensure_loaded(&addr).await?;
        if !self.state(&addr)?.is_idle() {
            return Err(SubsystemError::conflict(
                ConflictKind::AgentBusy,
                &addr.agent_uuid,
            ));
        }
        self.handles.storage.delete_agent(addr.clone()).await?;
        self.entries.remove(&addr);
        self.emit_workspace_event(
            &addr.workspace_uuid,
            WsEvent::AgentDeleted {
                agent_uuid: addr.agent_uuid,
            },
        );
        Ok(())
    }

    fn forget_workspace(&mut self, workspace_uuid: &str) -> SubsystemResult<()> {
        self.entries
            .retain(|addr, _| addr.workspace_uuid != workspace_uuid);
        Ok(())
    }

    fn try_shutdown(&self) -> SubsystemResult<bool> {
        // Workflow work is already represented by an agent's non-idle status:
        // its final tool completion transitions directly into the final LLM output.
        Ok(self.entries.values().all(|entry| entry.state.is_idle()))
    }

    async fn contains(&mut self, addr: &AgentAddr) -> SubsystemResult<bool> {
        match self.ensure_loaded(addr).await {
            Ok(()) => Ok(true),
            Err(SubsystemError::NotFound {
                resource: ResourceKind::Agent,
                ..
            }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    // Request handlers may load from disk; late background completions must only
    // inspect an existing entry and match their inference/job ID.
    pub(super) async fn ensure_loaded(&mut self, addr: &AgentAddr) -> SubsystemResult<()> {
        if !self.entries.contains_key(addr) {
            let agent = self.handles.storage.read_agent(addr.clone()).await?;
            self.entries.insert(addr.clone(), AgentEntry::idle(agent));
        }
        Ok(())
    }

    async fn snapshot(&mut self, addr: &AgentAddr) -> SubsystemResult<AgentSnapshot> {
        self.ensure_loaded(addr).await?;
        let agent = self.agent(addr)?;
        let workspace_uuid = addr.workspace_uuid.as_str();
        let units = if agent.unit_chain.is_empty() {
            Vec::new()
        } else {
            self.handles
                .storage
                .read_units(workspace_uuid, agent.unit_chain.clone())
                .await?
        };
        let state = self.state(addr)?;
        Ok(AgentSnapshot {
            units,
            status: state.status(),
            pending_approval: pending_approval_from_state(state),
        })
    }

    async fn send_message(&mut self, request: SendMessageRequest) -> SubsystemResult<()> {
        self.ensure_loaded(&request.addr).await?;
        if !self.state(&request.addr)?.is_idle() {
            return Err(SubsystemError::conflict(
                ConflictKind::AgentBusy,
                request.addr.to_string(),
            ));
        }
        let addr = request.addr.clone();
        self.apply_next_action(
            &addr,
            NextAction::StartInference {
                request,
                turn: TurnContext::default(),
            },
        )
    }

    async fn self_update(&mut self, request: SelfUpdateRequest) -> SubsystemResult<Agent> {
        self.ensure_loaded(&request.addr).await?;
        if request.context_refs.is_none()
            && request.context_out.is_none()
            && request.auto_loop.is_none()
            && request.auto_loop_message.is_none()
        {
            return Err(SubsystemError::validation(
                "self_update requires at least one field",
            ));
        }
        if request.auto_loop == Some(true) {
            return Err(SubsystemError::validation_field(
                "auto_loop",
                "self_update only supports setting auto_loop to false; use prismagent_task_finish for normal task completion",
            ));
        }
        let agent = self
            .handles
            .storage
            .update_agent(StorageAgentUpdateRequest {
                addr: request.addr.clone(),
                context_refs: request.context_refs,
                context_out: request.context_out,
                auto_loop: request.auto_loop,
                auto_loop_message: request.auto_loop_message,
            })
            .await?;
        self.entry_mut(&request.addr)?.agent = agent.clone();
        let summary = self.agent_summary(
            self.entries
                .get(&request.addr)
                .expect("entry checked above"),
        );
        self.emit_workspace_event(
            &request.addr.workspace_uuid,
            WsEvent::AgentUpdated { agent: summary },
        );
        Ok(agent)
    }

    pub(super) async fn commit_units(
        &mut self,
        addr: &AgentAddr,
        units: Vec<Unit>,
    ) -> SubsystemResult<()> {
        if units.is_empty() {
            return Ok(());
        }
        let updated_agent = self
            .handles
            .storage
            .append_agent_units(addr.clone(), units.clone())
            .await?;
        self.entry_mut(addr)?.agent = updated_agent;
        for unit in units {
            self.emit_agent_event(addr, WsEvent::UnitAppend { unit });
        }
        Ok(())
    }

    async fn set_auto_loop(&mut self, addr: &AgentAddr, enabled: bool) -> SubsystemResult<Agent> {
        self.ensure_loaded(addr).await?;
        if enabled {
            return Err(SubsystemError::validation_field(
                "auto_loop",
                "set_auto_loop(true) is not supported yet",
            ));
        }
        let agent = self
            .handles
            .storage
            .set_agent_auto_loop(addr.clone(), enabled)
            .await?;
        self.entry_mut(addr)?.agent = agent.clone();
        let summary = self.agent_summary(self.entries.get(addr).expect("entry checked above"));
        self.emit_workspace_event(
            &addr.workspace_uuid,
            WsEvent::AgentUpdated { agent: summary },
        );
        Ok(agent)
    }

    pub(super) fn emit_agent_event(&self, addr: &AgentAddr, event: WsEvent) {
        // Intentionally best-effort: ShellActor can be awaiting an AgentActor
        // request, so awaiting a full Shell mailbox here could create a
        // ShellActor <-> AgentActor wait cycle.
        let _ = self.handles.shell.emit_agent_event(addr.clone(), event);
    }

    /// Converts orchestration context plus an internal source error into the
    /// public asynchronous failure event consumed by web clients.
    pub(super) fn emit_task_failure(
        &self,
        addr: &AgentAddr,
        correlation_id: &str,
        error: AgentTaskError,
    ) {
        let AgentTaskError { stage, source } = error;
        self.emit_agent_event(
            addr,
            WsEvent::OperationFailed {
                workspace_uuid: Some(addr.workspace_uuid.clone()),
                agent_uuid: addr.agent_uuid.clone(),
                correlation_id: correlation_id.to_string(),
                stage,
                error: source.public_error(),
            },
        );
    }

    pub(super) fn emit_workspace_event(&self, workspace_uuid: &str, event: WsEvent) {
        // Keep cross-actor event emission non-blocking for the same reason as
        // emit_agent_event: ShellActor may currently be awaiting this actor.
        let _ = self
            .handles
            .shell
            .emit_workspace_event(workspace_uuid.to_string(), event);
    }

    fn agent_summary(&self, entry: &AgentEntry) -> AgentSummary {
        let agent = &entry.agent;
        AgentSummary {
            agent_uuid: agent.uuid.clone(),
            agent_name: agent.name.clone(),
            profile: agent.profile.clone(),
            auto_loop: agent.auto_loop,
            context_refs: agent.context_refs.clone(),
            context_out: agent.context_out.clone(),
            status: entry.state.status(),
        }
    }

    pub(super) fn agent(&self, addr: &AgentAddr) -> SubsystemResult<&Agent> {
        self.entries
            .get(addr)
            .map(|entry| &entry.agent)
            .ok_or_else(|| SubsystemError::not_found(ResourceKind::Agent, addr.to_string()))
    }

    pub(super) fn state(&self, addr: &AgentAddr) -> SubsystemResult<&AgentState> {
        self.entries
            .get(addr)
            .map(|entry| &entry.state)
            .ok_or_else(|| {
                SubsystemError::internal(
                    "access agent state",
                    format!("state is missing for agent {addr}"),
                )
            })
    }

    pub(super) fn entry_mut(&mut self, addr: &AgentAddr) -> SubsystemResult<&mut AgentEntry> {
        self.entries.get_mut(addr).ok_or_else(|| {
            SubsystemError::internal(
                "access agent entry",
                format!("entry is missing for agent {addr}"),
            )
        })
    }
}

// ---- Declarative macro: handle methods with concrete types ----

impl_handle_methods! {
    AgentHandle for AgentMsg, AGENT_ACTOR;

    fn try_shutdown(&self) -> bool
        => TryShutdown {};

    fn list(&self, workspace_uuid: impl Into<String>) -> Vec<AgentSummary>
        => List { workspace_uuid: workspace_uuid.into() };

    fn create(&self, request: AgentCreateRequest) -> Agent
        => Create { request: request };

    fn delete(&self, addr: AgentAddr) -> ()
        => Delete { addr: addr };

    fn forget_workspace(&self, workspace_uuid: impl Into<String>) -> ()
        => ForgetWorkspace { workspace_uuid: workspace_uuid.into() };

    fn contains(&self, addr: AgentAddr) -> bool
        => Contains { addr: addr };

    fn snapshot(&self, addr: AgentAddr) -> AgentSnapshot
        => Snapshot { addr: addr };

    fn cancel(&self, addr: AgentAddr) -> ()
        => Cancel { addr: addr };

    fn send_message(&self, request: SendMessageRequest) -> ()
        => SendMessage { request: request };

    fn self_update(&self, request: SelfUpdateRequest) -> Agent
        => SelfUpdate { request: request };

    fn set_auto_loop(&self, addr: AgentAddr, enabled: bool) -> Agent
        => SetAutoLoop { addr: addr, enabled: enabled };

    fn approve_request(&self, request: ApproveRequest) -> ()
        => ApproveRequest { request: request };
}

// ---- Manual handle methods (fire-and-forget: no reply channel) ----

impl AgentHandle {
    pub async fn inference_complete(
        &self,
        addr: AgentAddr,
        inference_uuid: impl Into<String>,
        result: AgentTaskResult<AgentInferenceOutput>,
    ) -> SubsystemResult<()> {
        self.tx
            .send(AgentMsg::InferenceFinished {
                addr,
                inference_uuid: inference_uuid.into(),
                result,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))
    }

    pub async fn tool_batch_complete(
        &self,
        addr: AgentAddr,
        job_uuid: impl Into<String>,
        result: AgentTaskResult<ToolBatchOutput>,
    ) -> SubsystemResult<()> {
        self.tx
            .send(AgentMsg::ToolBatchFinished {
                addr,
                job_uuid: job_uuid.into(),
                result,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))
    }
}

pub(super) fn effective_approval_mask(
    auto_approved_mask: u64,
    manual_approval_mask: u64,
    user_approval_mask: u64,
) -> ApprovalMask {
    ApprovalMask::from_bits(auto_approved_mask | (user_approval_mask & manual_approval_mask))
}

fn pending_approval_from_state(state: &AgentState) -> Option<PendingApproval> {
    match state {
        AgentState::WaitingApproval {
            request_uuid,
            tool_calls,
            auto_approved_mask,
            manual_approval_mask,
            ..
        } => Some(PendingApproval {
            request_uuid: request_uuid.clone(),
            description: "model requested tool execution".to_string(),
            tool_count: tool_calls.len(),
            auto_approved_mask: *auto_approved_mask,
            manual_approval_mask: *manual_approval_mask,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod approval_tests {
    use super::*;

    #[test]
    fn approval_mask_zero_denies_all() {
        let mask = ApprovalMask::from_bits(0);

        assert!(!mask.approves(0));
        assert!(!mask.approves_all(1));
    }

    #[test]
    fn approval_mask_uses_one_bit_per_tool_call() {
        let mask = ApprovalMask::from_bits(0b111);

        assert!(mask.approves(0));
        assert!(mask.approves(1));
        assert!(mask.approves(2));
        assert!(mask.approves_all(3));
        assert!(!mask.approves_all(4));
    }

    #[test]
    fn user_approval_mask_is_limited_to_manual_bits() {
        let mask = effective_approval_mask(0b010, 0b111, 0b001);

        assert!(mask.approves(0));
        assert!(mask.approves(1));
        assert!(!mask.approves(2));
        assert!(!mask.approves_all(3));
    }

    #[test]
    fn effective_mask_continues_only_when_all_tools_are_approved() {
        let mask = effective_approval_mask(0b010, 0b101, 0b101);

        assert!(mask.approves_all(3));
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::actors::agent_actor::model::AgentStatus;
    use std::collections::HashMap;

    fn test_agent(agent_uuid: &str) -> Agent {
        Agent {
            uuid: agent_uuid.to_string(),
            name: agent_uuid.to_string(),
            profile: "default".to_string(),
            auto_loop: false,
            auto_loop_message: String::new(),
            unit_chain: Vec::new(),
            unit_head: String::new(),
            context_refs: Vec::new(),
            context_out: Vec::new(),
            snapshots: HashMap::new(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn forget_workspace_removes_only_its_cached_agents() {
        let (_tx, rx) = mpsc::channel(1);
        let mut actor = AgentActor::load(rx, crate::handles::test_handles());
        actor.entries.insert(
            AgentAddr::new("workspace-a", "same"),
            AgentEntry::idle(test_agent("same")),
        );
        actor.entries.insert(
            AgentAddr::new("workspace-b", "same"),
            AgentEntry::idle(test_agent("same")),
        );

        actor.forget_workspace("workspace-a").unwrap();

        assert!(
            !actor
                .entries
                .contains_key(&AgentAddr::new("workspace-a", "same"))
        );
        assert!(
            actor
                .entries
                .contains_key(&AgentAddr::new("workspace-b", "same"))
        );
    }

    #[tokio::test]
    async fn contains_and_delete_use_the_full_address() {
        use crate::actors::storage_actor::model::{StorageActor, StorageHandle};

        let root = std::env::temp_dir().join(format!("agent-delete-{}", uuid::Uuid::now_v7()));
        let (tx, rx) = mpsc::channel(8);
        StorageActor::from_root(rx, root.clone()).unwrap().spawn();
        let mut handles = crate::handles::test_handles();
        handles.storage = StorageHandle { tx };
        let (tx, rx) = mpsc::channel(8);
        let handle = AgentHandle { tx };
        AgentActor::load(rx, handles).spawn();
        let a = AgentAddr::new("a", "same");
        let b = AgentAddr::new("b", "same");
        for addr in [&a, &b] {
            let dir = root.join(&addr.workspace_uuid).join("agents");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("same.json"),
                serde_json::to_vec(&test_agent("same")).unwrap(),
            )
            .unwrap();
        }
        // The planner can update context after restart without listing agents first.
        let updated = handle
            .self_update(SelfUpdateRequest {
                addr: a.clone(),
                context_refs: None,
                context_out: Some(vec!["out".into()]),
                auto_loop: None,
                auto_loop_message: None,
            })
            .await
            .unwrap();
        assert_eq!(updated.context_out, ["out"]);
        assert!(handle.contains(a.clone()).await.unwrap());
        assert!(handle.snapshot(b.clone()).await.unwrap().units.is_empty());
        handle.delete(a.clone()).await.unwrap();
        assert!(!handle.contains(a).await.unwrap());
        assert!(handle.contains(b).await.unwrap());
        assert!(!root.join("a/agents/same.json").exists());
        assert!(root.join("b/agents/same.json").is_file());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn cold_send_and_completion_are_scoped_to_one_workspace() {
        use crate::actors::agent_actor::model::MessageBody;
        use crate::actors::storage_actor::model::{StorageActor, StorageHandle};

        let root = std::env::temp_dir().join(format!("agent-routing-{}", uuid::Uuid::now_v7()));
        let (tx, rx) = mpsc::channel(8);
        StorageActor::from_root(rx, root.clone()).unwrap().spawn();
        let mut handles = crate::handles::test_handles();
        handles.storage = StorageHandle { tx };
        let (_tx, rx) = mpsc::channel(8);
        let mut actor = AgentActor::load(rx, handles);
        let a = AgentAddr::new("workspace-a", "same");
        let b = AgentAddr::new("workspace-b", "same");
        for addr in [&a, &b] {
            let dir = root.join(&addr.workspace_uuid).join("agents");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("same.json"),
                serde_json::to_vec(&test_agent("same")).unwrap(),
            )
            .unwrap();
            actor
                .send_message(SendMessageRequest {
                    addr: addr.clone(),
                    message_body: MessageBody {
                        text: "hello".into(),
                        attachments: vec![],
                    },
                })
                .await
                .unwrap();
        }
        let AgentState::RunningLlm { inference_uuid, .. } = actor.state(&a).unwrap().clone() else {
            panic!("expected running inference")
        };
        let unit = Unit::from_user_text("only workspace A".into());
        actor
            .finish_inference(
                &a,
                &inference_uuid,
                Ok(AgentInferenceOutput {
                    units: vec![unit.clone()],
                    is_tool_calls: false,
                }),
            )
            .await;
        assert_eq!(actor.snapshot(&a).await.unwrap().units[0].uuid, unit.uuid);
        assert!(actor.snapshot(&b).await.unwrap().units.is_empty());
        assert_eq!(actor.state(&b).unwrap().status(), AgentStatus::RunningLlm);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn try_shutdown_requires_every_agent_state_to_be_idle() {
        let (_tx, rx) = mpsc::channel(1);
        let mut actor = AgentActor::load(rx, crate::handles::test_handles());

        assert!(actor.try_shutdown().unwrap());

        actor.entries.insert(
            AgentAddr::new("workspace-a", "same"),
            AgentEntry::idle(test_agent("same")),
        );
        assert!(actor.try_shutdown().unwrap());

        actor
            .entries
            .get_mut(&AgentAddr::new("workspace-a", "same"))
            .unwrap()
            .state = AgentState::RunningLlm {
            inference_uuid: "inference".to_string(),
            turn: TurnContext::default(),
        };
        assert!(!actor.try_shutdown().unwrap());
        assert_eq!(
            actor.entries[&AgentAddr::new("workspace-a", "same")]
                .state
                .status(),
            AgentStatus::RunningLlm
        );
    }
}
