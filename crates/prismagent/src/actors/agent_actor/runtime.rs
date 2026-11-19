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
                    AgentMsg::Delete { workspace_uuid, agent_uuid ; reply } => self.delete(&workspace_uuid, &agent_uuid).await,
                    AgentMsg::ForgetWorkspace { workspace_uuid ; reply } => self.forget_workspace(&workspace_uuid),
                    AgentMsg::Contains { workspace_uuid, agent_uuid ; reply } => Ok(self.contains(&workspace_uuid, &agent_uuid)),
                    AgentMsg::Snapshot { agent_uuid ; reply } => self.snapshot(&agent_uuid).await,
                    AgentMsg::SendMessage { request ; reply } => self.send_message(request).await,
                    AgentMsg::SelfUpdate { request ; reply } => self.self_update(request).await,
                    AgentMsg::ApproveRequest { request ; reply } => self.approve_request(request).await,
                    AgentMsg::Cancel { agent_uuid ; reply } => self.cancel(&agent_uuid).await,
                    AgentMsg::SetAutoLoop { agent_uuid, enabled ; reply } => self.set_auto_loop(&agent_uuid, enabled).await,
                }
                fire {
                    AgentMsg::InferenceFinished { agent_uuid, inference_uuid, result } => self.finish_inference(&agent_uuid, &inference_uuid, result).await,
                    AgentMsg::ToolBatchFinished { agent_uuid, job_uuid, result } => self.finish_tool_batch(&agent_uuid, &job_uuid, result).await,
                }
            );
        }
    }

    async fn list(&mut self, workspace_uuid: &str) -> SubsystemResult<Vec<AgentSummary>> {
        let uuids = self.handles.storage.list_agents(workspace_uuid).await?;
        let uncached = uuids
            .into_iter()
            .filter(|uuid| !self.entries.contains_key(uuid))
            .collect::<Vec<_>>();
        let discovered = self
            .handles
            .storage
            .read_agents(workspace_uuid, uncached)
            .await?;
        for agent in discovered {
            self.entries.insert(
                agent.uuid.clone(),
                AgentEntry::idle(workspace_uuid.to_string(), agent),
            );
        }
        let mut agents = self
            .entries
            .values()
            .filter(|entry| entry.workspace_uuid == workspace_uuid)
            .map(|entry| self.agent_summary(entry))
            .collect::<Vec<_>>();
        agents.sort_by(|left, right| left.agent_name.cmp(&right.agent_name));
        Ok(agents)
    }

    async fn create(&mut self, request: AgentCreateRequest) -> SubsystemResult<Agent> {
        let workspace_uuid = request.workspace_uuid.clone();
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
                workspace_uuid: workspace_uuid.clone(),
                agent_uuid: request.uuid.clone(),
                context_refs,
                profile,
            }))
            .await?;
        let mut agent = self
            .handles
            .storage
            .create_agent(request, auto_loop, auto_loop_message)
            .await?;
        if !initial_units.is_empty() {
            agent = self
                .handles
                .storage
                .append_agent_units(workspace_uuid.clone(), agent.uuid.clone(), initial_units)
                .await?;
        }
        self.entries.insert(
            agent.uuid.clone(),
            AgentEntry::idle(workspace_uuid.clone(), agent.clone()),
        );
        let summary = self.agent_summary(
            self.entries
                .get(&agent.uuid)
                .expect("agent entry inserted above"),
        );
        self.emit_workspace_event(&workspace_uuid, WsEvent::AgentCreated { agent: summary });
        if has_initial_task && auto_loop {
            self.apply_next_action(
                &agent.uuid,
                NextAction::ContinueInference {
                    turn: TurnContext::default(),
                },
            )?;
        }
        Ok(agent)
    }

    async fn delete(&mut self, workspace_uuid: &str, agent_uuid: &str) -> SubsystemResult<()> {
        if !self.contains(workspace_uuid, agent_uuid) {
            return Err(SubsystemError::not_found(ResourceKind::Agent, agent_uuid));
        }
        if !self.state(agent_uuid)?.is_idle() {
            return Err(SubsystemError::conflict(
                ConflictKind::AgentBusy,
                agent_uuid,
            ));
        }
        self.handles
            .storage
            .delete_agent(workspace_uuid.to_string(), agent_uuid.to_string())
            .await?;
        self.entries.remove(agent_uuid);
        self.emit_workspace_event(
            workspace_uuid,
            WsEvent::AgentDeleted {
                agent_uuid: agent_uuid.to_string(),
            },
        );
        Ok(())
    }

    fn forget_workspace(&mut self, workspace_uuid: &str) -> SubsystemResult<()> {
        self.entries
            .retain(|_, entry| entry.workspace_uuid != workspace_uuid);
        Ok(())
    }

    fn try_shutdown(&self) -> SubsystemResult<bool> {
        // Workflow work is already represented by an agent's non-idle status:
        // its final tool completion transitions directly into the final LLM output.
        Ok(self.entries.values().all(|entry| entry.state.is_idle()))
    }

    fn contains(&self, workspace_uuid: &str, agent_uuid: &str) -> bool {
        self.entries
            .get(agent_uuid)
            .is_some_and(|entry| entry.workspace_uuid == workspace_uuid)
    }

    async fn snapshot(&self, agent_uuid: &str) -> SubsystemResult<AgentSnapshot> {
        let agent = self.agent(agent_uuid)?;
        let workspace_uuid = self.workspace_uuid(agent_uuid)?;
        let units = if agent.unit_chain.is_empty() {
            Vec::new()
        } else {
            self.handles
                .storage
                .read_units(workspace_uuid, agent.unit_chain.clone())
                .await?
        };
        let state = self.state(agent_uuid)?;
        Ok(AgentSnapshot {
            units,
            status: state.status(),
            pending_approval: pending_approval_from_state(state),
        })
    }

    async fn send_message(&mut self, request: SendMessageRequest) -> SubsystemResult<()> {
        if !self.state(&request.agent_uuid)?.is_idle() {
            return Err(SubsystemError::conflict(
                ConflictKind::AgentBusy,
                request.agent_uuid,
            ));
        }
        let agent_uuid = request.agent_uuid.clone();
        self.apply_next_action(
            &agent_uuid,
            NextAction::StartInference {
                request,
                turn: TurnContext::default(),
            },
        )
    }

    async fn self_update(&mut self, request: SelfUpdateRequest) -> SubsystemResult<Agent> {
        self.agent(&request.agent_uuid)?;
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
        let workspace_uuid = self.workspace_uuid(&request.agent_uuid)?.to_string();
        let agent = self
            .handles
            .storage
            .update_agent(StorageAgentUpdateRequest {
                workspace_uuid: workspace_uuid.clone(),
                agent_uuid: request.agent_uuid.clone(),
                context_refs: request.context_refs,
                context_out: request.context_out,
                auto_loop: request.auto_loop,
                auto_loop_message: request.auto_loop_message,
            })
            .await?;
        self.entry_mut(&agent.uuid)?.agent = agent.clone();
        let summary =
            self.agent_summary(self.entries.get(&agent.uuid).expect("entry checked above"));
        self.emit_workspace_event(&workspace_uuid, WsEvent::AgentUpdated { agent: summary });
        Ok(agent)
    }

    pub(super) async fn commit_units(
        &mut self,
        agent_uuid: &str,
        units: Vec<Unit>,
    ) -> SubsystemResult<()> {
        if units.is_empty() {
            return Ok(());
        }
        let workspace_uuid = self.workspace_uuid(agent_uuid)?.to_string();
        let updated_agent = self
            .handles
            .storage
            .append_agent_units(workspace_uuid, agent_uuid.to_string(), units.clone())
            .await?;
        self.entry_mut(agent_uuid)?.agent = updated_agent;
        for unit in units {
            self.emit_agent_event(agent_uuid, WsEvent::UnitAppend { unit });
        }
        Ok(())
    }

    async fn set_auto_loop(&mut self, agent_uuid: &str, enabled: bool) -> SubsystemResult<Agent> {
        if enabled {
            return Err(SubsystemError::validation_field(
                "auto_loop",
                "set_auto_loop(true) is not supported yet",
            ));
        }
        let workspace_uuid = self.workspace_uuid(agent_uuid)?.to_string();
        let agent = self
            .handles
            .storage
            .set_agent_auto_loop(workspace_uuid.clone(), agent_uuid.to_string(), enabled)
            .await?;
        self.entry_mut(agent_uuid)?.agent = agent.clone();
        let summary =
            self.agent_summary(self.entries.get(agent_uuid).expect("entry checked above"));
        self.emit_workspace_event(&workspace_uuid, WsEvent::AgentUpdated { agent: summary });
        Ok(agent)
    }

    pub(super) fn emit_agent_event(&self, agent_uuid: &str, event: WsEvent) {
        // Intentionally best-effort: ShellActor can be awaiting an AgentActor
        // request, so awaiting a full Shell mailbox here could create a
        // ShellActor <-> AgentActor wait cycle.
        let _ = self
            .handles
            .shell
            .emit_agent_event(agent_uuid.to_string(), event);
    }

    /// Converts orchestration context plus an internal source error into the
    /// public asynchronous failure event consumed by web clients.
    pub(super) fn emit_task_failure(
        &self,
        agent_uuid: &str,
        correlation_id: &str,
        error: AgentTaskError,
    ) {
        let AgentTaskError { stage, source } = error;
        self.emit_agent_event(
            agent_uuid,
            WsEvent::OperationFailed {
                workspace_uuid: self.workspace_uuid(agent_uuid).ok().map(str::to_string),
                agent_uuid: agent_uuid.to_string(),
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

    pub(super) fn agent(&self, agent_uuid: &str) -> SubsystemResult<&Agent> {
        self.entries
            .get(agent_uuid)
            .map(|entry| &entry.agent)
            .ok_or_else(|| SubsystemError::not_found(ResourceKind::Agent, agent_uuid))
    }

    pub(super) fn state(&self, agent_uuid: &str) -> SubsystemResult<&AgentState> {
        self.entries
            .get(agent_uuid)
            .map(|entry| &entry.state)
            .ok_or_else(|| {
                SubsystemError::internal(
                    "access agent state",
                    format!("state is missing for agent {agent_uuid}"),
                )
            })
    }

    pub(super) fn entry_mut(&mut self, agent_uuid: &str) -> SubsystemResult<&mut AgentEntry> {
        self.entries.get_mut(agent_uuid).ok_or_else(|| {
            SubsystemError::internal(
                "access agent entry",
                format!("entry is missing for agent {agent_uuid}"),
            )
        })
    }

    pub(super) fn workspace_uuid(&self, agent_uuid: &str) -> SubsystemResult<&str> {
        self.entries
            .get(agent_uuid)
            .map(|entry| entry.workspace_uuid.as_str())
            .ok_or_else(|| {
                SubsystemError::internal(
                    "resolve agent workspace",
                    format!("workspace mapping is missing for agent {agent_uuid}"),
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

    fn delete(&self, workspace_uuid: impl Into<String>, agent_uuid: impl Into<String>) -> ()
        => Delete { workspace_uuid: workspace_uuid.into(), agent_uuid: agent_uuid.into() };

    fn forget_workspace(&self, workspace_uuid: impl Into<String>) -> ()
        => ForgetWorkspace { workspace_uuid: workspace_uuid.into() };

    fn contains(&self, workspace_uuid: impl Into<String>, agent_uuid: impl Into<String>) -> bool
        => Contains { workspace_uuid: workspace_uuid.into(), agent_uuid: agent_uuid.into() };

    fn snapshot(&self, agent_uuid: impl Into<String>) -> AgentSnapshot
        => Snapshot { agent_uuid: agent_uuid.into() };

    fn cancel(&self, agent_uuid: impl Into<String>) -> ()
        => Cancel { agent_uuid: agent_uuid.into() };

    fn send_message(&self, request: SendMessageRequest) -> ()
        => SendMessage { request: request };

    fn self_update(&self, request: SelfUpdateRequest) -> Agent
        => SelfUpdate { request: request };

    fn set_auto_loop(&self, agent_uuid: impl Into<String>, enabled: bool) -> Agent
        => SetAutoLoop { agent_uuid: agent_uuid.into(), enabled: enabled };

    fn approve_request(&self, request: ApproveRequest) -> ()
        => ApproveRequest { request: request };
}

// ---- Manual handle methods (fire-and-forget: no reply channel) ----

impl AgentHandle {
    pub async fn inference_complete(
        &self,
        agent_uuid: impl Into<String>,
        inference_uuid: impl Into<String>,
        result: AgentTaskResult<AgentInferenceOutput>,
    ) -> SubsystemResult<()> {
        self.tx
            .send(AgentMsg::InferenceFinished {
                agent_uuid: agent_uuid.into(),
                inference_uuid: inference_uuid.into(),
                result,
            })
            .await
            .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))
    }

    pub async fn tool_batch_complete(
        &self,
        agent_uuid: impl Into<String>,
        job_uuid: impl Into<String>,
        result: AgentTaskResult<ToolBatchOutput>,
    ) -> SubsystemResult<()> {
        self.tx
            .send(AgentMsg::ToolBatchFinished {
                agent_uuid: agent_uuid.into(),
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
            "agent-a".to_string(),
            AgentEntry::idle("workspace-a".to_string(), test_agent("agent-a")),
        );
        actor.entries.insert(
            "agent-b".to_string(),
            AgentEntry::idle("workspace-b".to_string(), test_agent("agent-b")),
        );

        actor.forget_workspace("workspace-a").unwrap();

        assert!(!actor.entries.contains_key("agent-a"));
        assert_eq!(
            actor
                .entries
                .get("agent-b")
                .map(|entry| entry.workspace_uuid.as_str()),
            Some("workspace-b")
        );
    }

    #[test]
    fn try_shutdown_requires_every_agent_state_to_be_idle() {
        let (_tx, rx) = mpsc::channel(1);
        let mut actor = AgentActor::load(rx, crate::handles::test_handles());

        assert!(actor.try_shutdown().unwrap());

        actor.entries.insert(
            "agent-a".to_string(),
            AgentEntry::idle("workspace-a".to_string(), test_agent("agent-a")),
        );
        assert!(actor.try_shutdown().unwrap());

        actor.entries.get_mut("agent-a").unwrap().state = AgentState::RunningLlm {
            inference_uuid: "inference".to_string(),
            turn: TurnContext::default(),
        };
        assert!(!actor.try_shutdown().unwrap());
        assert_eq!(
            actor.entries["agent-a"].state.status(),
            AgentStatus::RunningLlm
        );
    }
}
