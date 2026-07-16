use crate::actors::agent_actor::model::{
    AGENT_ACTOR, AgentActor, AgentHandle, AgentInferenceOutput, AgentMsg, AgentRuntime,
    AgentSnapshot, AgentStatus, AgentSummary, AgentTaskError, AgentTaskOperation, AgentTaskPhase,
    AgentTaskResult, ApproveRequest, PendingApproval, PendingToolBatch, SelfUpdateRequest,
    SendMessageRequest, ToolBatchOutput,
};
use crate::actors::agent_actor::pipeline::{
    RunToolBatchRequest, auto_approval_mask, clone_tool_calls, run_llm_continuation,
    run_llm_inference, run_tool_batch, tool_batch_is_auto_approved, tool_calls_sound,
    tool_response_units,
};
use crate::actors::context_actor::model::RenderInitialPromptsRequest;
use crate::actors::shell_actor::model::WsEvent;
use crate::actors::storage_actor::model::agent::{
    Agent, AgentCreateRequest, AgentUpdateRequest as StorageAgentUpdateRequest,
};
use crate::actors::storage_actor::model::unit::{Unit, UnitVisibility};
use crate::error::{
    ConflictKind, ErrorClass, ExternalKind, ResourceKind, SubsystemError, SubsystemResult,
};
use crate::handles::AppHandles;
use crate::{actor_dispatch_mixed, impl_handle_methods};
use genai::chat::ToolCall;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

const MAX_MALFORMED_TOOL_CALL_RETRIES: u8 = 2;

impl AgentActor {
    pub fn load(rx: mpsc::Receiver<AgentMsg>, handles: AppHandles) -> Self {
        Self {
            rx,
            agents: HashMap::new(),
            agent_workspace: HashMap::new(),
            runtimes: HashMap::new(),
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
                    AgentMsg::InferenceFinished { agent_uuid, inference_uuid, operation, result } => self.finish_inference(&agent_uuid, &inference_uuid, operation, result).await,
                    AgentMsg::ToolBatchFinished { agent_uuid, job_uuid, result } => self.finish_tool_batch(&agent_uuid, &job_uuid, result).await,
                }
            );
        }
    }

    async fn list(&mut self, workspace_uuid: &str) -> SubsystemResult<Vec<AgentSummary>> {
        let uuids = self.handles.storage.list_agents(workspace_uuid).await?;
        let uncached = uuids
            .into_iter()
            .filter(|uuid| !self.agents.contains_key(uuid))
            .collect::<Vec<_>>();
        let discovered = self
            .handles
            .storage
            .read_agents(workspace_uuid, uncached)
            .await?;
        for agent in discovered {
            self.agent_workspace
                .insert(agent.uuid.clone(), workspace_uuid.to_string());
            self.runtimes
                .insert(agent.uuid.clone(), AgentRuntime::idle());
            self.agents.insert(agent.uuid.clone(), agent);
        }
        let mut agents = self
            .agents
            .values()
            .filter(|agent| {
                self.agent_workspace
                    .get(&agent.uuid)
                    .is_some_and(|candidate| candidate == workspace_uuid)
            })
            .map(|agent| self.agent_summary(agent))
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
        self.agent_workspace
            .insert(agent.uuid.clone(), workspace_uuid.clone());
        self.runtimes
            .insert(agent.uuid.clone(), AgentRuntime::idle());
        self.agents.insert(agent.uuid.clone(), agent.clone());
        self.emit_workspace_event(
            &workspace_uuid,
            WsEvent::AgentCreated {
                agent: self.agent_summary(&agent),
            },
        );
        if has_initial_task && auto_loop {
            self.spawn_llm_continuation(&agent.uuid).await?;
        }
        Ok(agent)
    }

    async fn delete(&mut self, workspace_uuid: &str, agent_uuid: &str) -> SubsystemResult<()> {
        if !self.contains(workspace_uuid, agent_uuid) {
            return Err(SubsystemError::not_found(ResourceKind::Agent, agent_uuid));
        }
        if self.runtime(agent_uuid)?.status != AgentStatus::Idle {
            return Err(SubsystemError::conflict(
                ConflictKind::AgentBusy,
                agent_uuid,
            ));
        }
        self.handles
            .storage
            .delete_agent(workspace_uuid.to_string(), agent_uuid.to_string())
            .await?;
        self.agents.remove(agent_uuid);
        self.agent_workspace.remove(agent_uuid);
        self.runtimes.remove(agent_uuid);
        self.emit_workspace_event(
            workspace_uuid,
            WsEvent::AgentDeleted {
                agent_uuid: agent_uuid.to_string(),
            },
        );
        Ok(())
    }

    fn forget_workspace(&mut self, workspace_uuid: &str) -> SubsystemResult<()> {
        let agent_uuids = self
            .agent_workspace
            .iter()
            .filter(|(_, mapped_workspace)| mapped_workspace.as_str() == workspace_uuid)
            .map(|(agent_uuid, _)| agent_uuid.clone())
            .collect::<Vec<_>>();
        for agent_uuid in agent_uuids {
            self.agents.remove(&agent_uuid);
            self.agent_workspace.remove(&agent_uuid);
            self.runtimes.remove(&agent_uuid);
        }
        Ok(())
    }

    fn contains(&self, workspace_uuid: &str, agent_uuid: &str) -> bool {
        self.agents.contains_key(agent_uuid)
            && self
                .agent_workspace
                .get(agent_uuid)
                .is_some_and(|candidate| candidate == workspace_uuid)
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
        let runtime = self.runtime(agent_uuid)?;
        Ok(AgentSnapshot {
            units,
            status: runtime.status.clone(),
            pending_approval: pending_approval_from_runtime(runtime),
        })
    }

    async fn send_message(&mut self, request: SendMessageRequest) -> SubsystemResult<()> {
        if self.runtime(&request.agent_uuid)?.status != AgentStatus::Idle {
            return Err(SubsystemError::conflict(
                ConflictKind::AgentBusy,
                request.agent_uuid,
            ));
        }
        let agent = self.agent(&request.agent_uuid)?;
        let unit_uuids = agent.unit_chain.clone();
        let profile_name = agent.profile.clone();
        let agent_uuid = request.agent_uuid.clone();
        let workspace_uuid = self.workspace_uuid(&agent_uuid)?.to_string();
        self.runtime_mut(&agent_uuid)?.malformed_tool_call_retries = 0;
        self.set_status(&agent_uuid, AgentStatus::RunningLlm)?;

        let handles = self.handles.clone();
        let task_agent_uuid = agent_uuid.clone();
        let inference_uuid = Uuid::now_v7().to_string();
        let task_inference_uuid = inference_uuid.clone();
        let request_inference_uuid = inference_uuid.clone();
        tokio::spawn(async move {
            let result = run_llm_inference(
                &handles,
                workspace_uuid,
                unit_uuids,
                request,
                profile_name,
                request_inference_uuid,
            )
            .await;
            let _ = handles
                .agent
                .inference_complete(
                    task_agent_uuid,
                    task_inference_uuid,
                    AgentTaskOperation::LlmInference,
                    result,
                )
                .await;
        });
        let runtime = self.runtime_mut(&agent_uuid)?;
        runtime.inference_uuid = Some(inference_uuid);
        Ok(())
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
        self.agents.insert(agent.uuid.clone(), agent.clone());
        self.emit_workspace_event(
            &workspace_uuid,
            WsEvent::AgentUpdated {
                agent: self.agent_summary(&agent),
            },
        );
        Ok(agent)
    }

    async fn finish_inference(
        &mut self,
        agent_uuid: &str,
        inference_uuid: &str,
        operation: AgentTaskOperation,
        result: AgentTaskResult<AgentInferenceOutput>,
    ) {
        let is_active = self
            .runtime(agent_uuid)
            .map(|runtime| runtime.inference_uuid.as_deref() == Some(inference_uuid))
            .unwrap_or(false);
        if !is_active {
            return;
        }
        let runtime = self.runtime_mut(agent_uuid).expect("runtime checked above");
        runtime.inference_uuid = None;
        match result {
            Ok(output) => {
                let is_tool_calls = output.is_tool_calls;
                let tool_calls = if is_tool_calls {
                    output
                        .units
                        .last()
                        .map(clone_tool_calls)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                if is_tool_calls && !tool_calls_sound(&tool_calls) {
                    if let Err(source) = self.handle_malformed_tool_calls(agent_uuid).await {
                        self.emit_task_failure(
                            agent_uuid,
                            inference_uuid,
                            AgentTaskError::new(operation, AgentTaskPhase::RepairToolCalls, source),
                        );
                        let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                    }
                    return;
                }
                self.runtime_mut(agent_uuid)
                    .expect("runtime checked above")
                    .malformed_tool_call_retries = 0;
                match self.commit_units(agent_uuid, output.units).await {
                    Ok(()) if is_tool_calls => {
                        if let Err(source) = self.handle_tool_calls(agent_uuid, tool_calls).await {
                            self.emit_task_failure(
                                agent_uuid,
                                inference_uuid,
                                AgentTaskError::new(
                                    operation,
                                    AgentTaskPhase::PrepareToolBatch,
                                    source,
                                ),
                            );
                            let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                        }
                        return;
                    }
                    Ok(()) => {
                        if self.agent(agent_uuid).is_ok_and(|agent| agent.auto_loop) {
                            if let Err(source) = self.spawn_auto_loop_continuation(agent_uuid).await
                            {
                                self.emit_task_failure(
                                    agent_uuid,
                                    inference_uuid,
                                    AgentTaskError::new(
                                        AgentTaskOperation::AutoLoop,
                                        AgentTaskPhase::ContinueLoop,
                                        source,
                                    ),
                                );
                                let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                            }
                            return;
                        }
                    }
                    Err(source) => {
                        self.emit_task_failure(
                            agent_uuid,
                            inference_uuid,
                            AgentTaskError::new(operation, AgentTaskPhase::CommitUnits, source),
                        );
                    }
                }
            }
            Err(error) => {
                debug_assert_eq!(error.operation, operation);
                self.emit_task_failure(agent_uuid, inference_uuid, error);
            }
        }
        let _ = self.set_status(agent_uuid, AgentStatus::Idle);
    }

    async fn handle_malformed_tool_calls(&mut self, agent_uuid: &str) -> SubsystemResult<()> {
        let retry = {
            let runtime = self.runtime_mut(agent_uuid)?;
            if runtime.malformed_tool_call_retries < MAX_MALFORMED_TOOL_CALL_RETRIES {
                runtime.malformed_tool_call_retries += 1;
                Some(runtime.malformed_tool_call_retries)
            } else {
                None
            }
        };

        let text = match retry {
            Some(attempt) => format!(
                "[PrismAgent] Your previous tool call was malformed. Tool arguments must be a JSON object, and call_id/fn_name must be non-empty. Retry the tool call with valid arguments. Repair attempt {attempt}/{MAX_MALFORMED_TOOL_CALL_RETRIES}."
            ),
            None => format!(
                "[PrismAgent] Tool-call repair stopped after {MAX_MALFORMED_TOOL_CALL_RETRIES} malformed attempts. Please inspect the previous output and continue manually."
            ),
        };
        self.commit_units(agent_uuid, vec![Unit::from_user_text(text)])
            .await?;

        if retry.is_some() {
            self.spawn_llm_continuation(agent_uuid).await
        } else {
            Err(SubsystemError::external(
                ExternalKind::Llm,
                ErrorClass::Internal,
                "LLM produced malformed tool calls repeatedly",
                false,
            ))
        }
    }

    async fn finish_tool_batch(
        &mut self,
        agent_uuid: &str,
        job_uuid: &str,
        result: AgentTaskResult<ToolBatchOutput>,
    ) {
        let is_active = self
            .runtime(agent_uuid)
            .map(|runtime| runtime.inference_uuid.as_deref() == Some(job_uuid))
            .unwrap_or(false);
        if !is_active {
            return;
        }
        let active_tool_batch = {
            let runtime = self.runtime_mut(agent_uuid).expect("runtime checked above");
            runtime.inference_uuid = None;
            runtime.active_tool_batch.take()
        };
        match result {
            Ok(output) => {
                let continue_loop = output.continue_loop;
                if let Err(source) = self.commit_units(agent_uuid, output.units).await {
                    self.emit_task_failure(
                        agent_uuid,
                        job_uuid,
                        AgentTaskError::new(
                            AgentTaskOperation::ToolBatch,
                            AgentTaskPhase::CommitUnits,
                            source,
                        ),
                    );
                    let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                    return;
                }
                if continue_loop {
                    if let Err(source) = self.spawn_llm_continuation(agent_uuid).await {
                        self.emit_task_failure(
                            agent_uuid,
                            job_uuid,
                            AgentTaskError::new(
                                AgentTaskOperation::ToolBatch,
                                AgentTaskPhase::ContinueLoop,
                                source,
                            ),
                        );
                        let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                    }
                } else {
                    let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                }
            }
            Err(error) => {
                if let Some(active) = active_tool_batch {
                    let units = tool_response_units(
                        &active.tool_calls,
                        "error",
                        &format!("tool batch failed: {}", error.source),
                    );
                    if let Err(source) = self.commit_units(agent_uuid, units).await {
                        self.emit_task_failure(
                            agent_uuid,
                            job_uuid,
                            AgentTaskError::new(
                                AgentTaskOperation::ToolBatch,
                                AgentTaskPhase::CommitUnits,
                                source,
                            ),
                        );
                    }
                }
                self.emit_task_failure(agent_uuid, job_uuid, error);
                let _ = self.set_status(agent_uuid, AgentStatus::Idle);
            }
        }
    }

    async fn commit_units(&mut self, agent_uuid: &str, units: Vec<Unit>) -> SubsystemResult<()> {
        if units.is_empty() {
            return Ok(());
        }
        let workspace_uuid = self.workspace_uuid(agent_uuid)?.to_string();
        let updated_agent = self
            .handles
            .storage
            .append_agent_units(workspace_uuid, agent_uuid.to_string(), units.clone())
            .await?;
        self.agents.insert(agent_uuid.to_string(), updated_agent);
        for unit in units {
            self.emit_agent_event(agent_uuid, WsEvent::UnitAppend { unit });
        }
        Ok(())
    }

    async fn approve_request(&mut self, request: ApproveRequest) -> SubsystemResult<()> {
        let pending = {
            let runtime = self.runtime_mut(&request.agent_uuid)?;
            let Some(pending) = runtime.pending_tool_batch.take() else {
                return Err(SubsystemError::validation(format!(
                    "no pending approval request: {}",
                    request.request_uuid
                )));
            };
            if pending.request_uuid != request.request_uuid {
                runtime.pending_tool_batch = Some(pending);
                return Err(SubsystemError::validation(format!(
                    "unknown approval request: {}",
                    request.request_uuid
                )));
            }
            pending
        };
        self.spawn_tool_batch(
            request.agent_uuid,
            pending.tool_calls,
            effective_approval_mask(
                pending.auto_approved_mask,
                pending.manual_approval_mask,
                request.approval_mask,
            ),
            "user denied tool execution",
        )
        .await
    }

    async fn cancel(&mut self, agent_uuid: &str) -> SubsystemResult<()> {
        match self.runtime(agent_uuid)?.status {
            AgentStatus::RunningLlm => {
                if let Some(inference_uuid) = self.runtime(agent_uuid)?.inference_uuid.clone() {
                    let _ = self.handles.llm.cancel(inference_uuid).await?;
                    self.runtime_mut(agent_uuid)?.inference_uuid = None;
                }
                self.set_status(agent_uuid, AgentStatus::Idle)?;
            }
            AgentStatus::WaitingApproval => {
                if let Some(pending) = self.runtime_mut(agent_uuid)?.pending_tool_batch.take() {
                    self.spawn_tool_batch(
                        agent_uuid.to_string(),
                        pending.tool_calls,
                        ApprovalMask::none(),
                        "tool execution cancelled by user",
                    )
                    .await?;
                }
            }
            AgentStatus::RunningTool => {
                let active = self.runtime_mut(agent_uuid)?.active_tool_batch.take();
                if let Some(active) = active {
                    let _ = self.handles.tools.cancel(active.request_uuid).await?;
                    self.runtime_mut(agent_uuid)?.inference_uuid = None;
                    self.spawn_tool_batch(
                        agent_uuid.to_string(),
                        active.tool_calls,
                        ApprovalMask::none(),
                        "tool execution cancelled by user",
                    )
                    .await?;
                }
            }
            AgentStatus::Idle => {}
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
        self.agents.insert(agent_uuid.to_string(), agent.clone());
        self.emit_workspace_event(
            &workspace_uuid,
            WsEvent::AgentUpdated {
                agent: self.agent_summary(&agent),
            },
        );
        Ok(agent)
    }

    async fn handle_tool_calls(
        &mut self,
        agent_uuid: &str,
        tool_calls: Vec<ToolCall>,
    ) -> SubsystemResult<()> {
        if tool_calls.is_empty() {
            self.set_status(agent_uuid, AgentStatus::Idle)?;
            return Ok(());
        }
        if tool_calls.len() > 64 {
            return Err(SubsystemError::validation(format!(
                "tool batch cannot contain more than 64 calls: {}",
                tool_calls.len()
            )));
        }
        let profile_name = self.agent(agent_uuid)?.profile.clone();
        let tools_config = self.handles.profile.tools(profile_name).await?;
        let all_mask = ApprovalMask::all_for(tool_calls.len()).bits();
        let auto_approved_mask = auto_approval_mask(&tools_config, &tool_calls) & all_mask;
        if tool_batch_is_auto_approved(&tools_config, &tool_calls) {
            self.spawn_tool_batch(
                agent_uuid.to_string(),
                tool_calls,
                ApprovalMask::from_bits(all_mask),
                "tool execution was auto-approved",
            )
            .await
        } else {
            let request_uuid = Uuid::now_v7().to_string();
            let tool_count = tool_calls.len();
            let manual_approval_mask = all_mask & !auto_approved_mask;
            self.runtime_mut(agent_uuid)?.pending_tool_batch = Some(PendingToolBatch {
                request_uuid: request_uuid.clone(),
                tool_calls,
                auto_approved_mask,
                manual_approval_mask,
            });
            self.set_status(agent_uuid, AgentStatus::WaitingApproval)?;
            self.emit_agent_event(
                agent_uuid,
                WsEvent::ApproveRequest {
                    request: PendingApproval {
                        request_uuid,
                        description: "model requested tool execution".to_string(),
                        tool_count,
                        auto_approved_mask,
                        manual_approval_mask,
                    },
                },
            );
            Ok(())
        }
    }

    async fn spawn_tool_batch(
        &mut self,
        agent_uuid: String,
        tool_calls: Vec<ToolCall>,
        approval_mask: ApprovalMask,
        denied_reason: &str,
    ) -> SubsystemResult<()> {
        let workspace_uuid = self.workspace_uuid(&agent_uuid)?.to_string();
        let profile_name = self.agent(&agent_uuid)?.profile.clone();
        let job_uuid = Uuid::now_v7().to_string();
        self.runtime_mut(&agent_uuid)?.inference_uuid = Some(job_uuid.clone());
        // Notice: auto_approved_mask&manual_approval_mask is useless since the approval result is already determined by approval_mask,
        // but we still use PendingToolBatch to store the info of active_tool_batch
        // so the value is set as 0 to avoid confusion.
        self.runtime_mut(&agent_uuid)?.active_tool_batch = Some(PendingToolBatch {
            request_uuid: job_uuid.clone(),
            tool_calls: tool_calls.clone(),
            auto_approved_mask: 0,
            manual_approval_mask: 0,
        });
        self.set_status(&agent_uuid, AgentStatus::RunningTool)?;
        let handles = self.handles.clone();
        let denied_reason = denied_reason.to_string();
        tokio::spawn(async move {
            let result = run_tool_batch(
                &handles,
                RunToolBatchRequest {
                    workspace_uuid,
                    agent_uuid: agent_uuid.clone(),
                    profile_name,
                    job_uuid: job_uuid.clone(),
                    tool_calls,
                    approval_mask,
                    denied_reason,
                },
            )
            .await;
            let _ = handles
                .agent
                .tool_batch_complete(agent_uuid, job_uuid, result)
                .await;
        });
        Ok(())
    }

    async fn spawn_llm_continuation(&mut self, agent_uuid: &str) -> SubsystemResult<()> {
        let agent = self.agent(agent_uuid)?;
        let unit_uuids = agent.unit_chain.clone();
        let profile_name = agent.profile.clone();
        let workspace_uuid = self.workspace_uuid(agent_uuid)?.to_string();
        let inference_uuid = Uuid::now_v7().to_string();
        self.runtime_mut(agent_uuid)?.inference_uuid = Some(inference_uuid.clone());
        self.set_status(agent_uuid, AgentStatus::RunningLlm)?;

        let handles = self.handles.clone();
        let task_agent_uuid = agent_uuid.to_string();
        let task_inference_uuid = inference_uuid.clone();
        tokio::spawn(async move {
            let result = run_llm_continuation(
                &handles,
                workspace_uuid,
                task_agent_uuid.clone(),
                unit_uuids,
                profile_name,
                task_inference_uuid.clone(),
            )
            .await;
            let _ = handles
                .agent
                .inference_complete(
                    task_agent_uuid,
                    task_inference_uuid,
                    AgentTaskOperation::LlmContinuation,
                    result,
                )
                .await;
        });
        Ok(())
    }

    async fn spawn_auto_loop_continuation(&mut self, agent_uuid: &str) -> SubsystemResult<()> {
        let auto_loop_message = self.agent(agent_uuid)?.auto_loop_message.clone();
        let mut unit = Unit::from_chat_message(genai::chat::ChatMessage::user(auto_loop_message));
        unit.visibility = UnitVisibility::Internal;
        self.commit_units(agent_uuid, vec![unit]).await?;
        self.spawn_llm_continuation(agent_uuid).await
    }

    fn set_status(&mut self, agent_uuid: &str, status: AgentStatus) -> SubsystemResult<()> {
        self.runtime_mut(agent_uuid)?.status = status.clone();
        self.emit_agent_event(
            agent_uuid,
            WsEvent::StatusChanged {
                status: status.clone(),
            },
        );
        if let Ok(workspace_uuid) = self.workspace_uuid(agent_uuid) {
            self.emit_workspace_event(
                workspace_uuid,
                WsEvent::AgentStatusChanged {
                    agent_uuid: agent_uuid.to_string(),
                    status,
                },
            );
        }
        Ok(())
    }

    fn emit_agent_event(&self, agent_uuid: &str, event: WsEvent) {
        let _ = self
            .handles
            .shell
            .emit_agent_event(agent_uuid.to_string(), event);
    }

    /// Converts orchestration context plus an internal source error into the
    /// public asynchronous failure event consumed by web clients.
    fn emit_task_failure(&self, agent_uuid: &str, correlation_id: &str, error: AgentTaskError) {
        let AgentTaskError {
            operation,
            phase,
            source,
        } = error;
        self.emit_agent_event(
            agent_uuid,
            WsEvent::OperationFailed {
                workspace_uuid: self.workspace_uuid(agent_uuid).ok().map(str::to_string),
                agent_uuid: agent_uuid.to_string(),
                correlation_id: correlation_id.to_string(),
                operation,
                phase,
                error: source.public_error(),
            },
        );
    }

    fn emit_workspace_event(&self, workspace_uuid: &str, event: WsEvent) {
        let _ = self
            .handles
            .shell
            .emit_workspace_event(workspace_uuid.to_string(), event);
    }

    fn agent_summary(&self, agent: &Agent) -> AgentSummary {
        AgentSummary {
            agent_uuid: agent.uuid.clone(),
            agent_name: agent.name.clone(),
            profile: agent.profile.clone(),
            auto_loop: agent.auto_loop,
            context_refs: agent.context_refs.clone(),
            context_out: agent.context_out.clone(),
            status: self
                .runtimes
                .get(&agent.uuid)
                .map(|runtime| runtime.status.clone())
                .unwrap_or(AgentStatus::Idle),
        }
    }

    fn agent(&self, agent_uuid: &str) -> SubsystemResult<&Agent> {
        self.agents
            .get(agent_uuid)
            .ok_or_else(|| SubsystemError::not_found(ResourceKind::Agent, agent_uuid))
    }

    fn runtime(&self, agent_uuid: &str) -> SubsystemResult<&AgentRuntime> {
        self.runtimes.get(agent_uuid).ok_or_else(|| {
            SubsystemError::internal(
                "access agent runtime",
                format!("runtime is missing for agent {agent_uuid}"),
            )
        })
    }

    fn runtime_mut(&mut self, agent_uuid: &str) -> SubsystemResult<&mut AgentRuntime> {
        self.runtimes.get_mut(agent_uuid).ok_or_else(|| {
            SubsystemError::internal(
                "access agent runtime",
                format!("runtime is missing for agent {agent_uuid}"),
            )
        })
    }

    fn workspace_uuid(&self, agent_uuid: &str) -> SubsystemResult<&str> {
        self.agent_workspace
            .get(agent_uuid)
            .map(String::as_str)
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
        operation: AgentTaskOperation,
        result: AgentTaskResult<AgentInferenceOutput>,
    ) -> SubsystemResult<()> {
        self.tx
            .send(AgentMsg::InferenceFinished {
                agent_uuid: agent_uuid.into(),
                inference_uuid: inference_uuid.into(),
                operation,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApprovalMask(u64);

impl ApprovalMask {
    pub fn none() -> Self {
        Self(0)
    }

    pub fn from_bits(mask: u64) -> Self {
        Self(mask)
    }

    pub fn all_for(len: usize) -> Self {
        debug_assert!(len <= 64);
        if len == 64 {
            Self(u64::MAX)
        } else if len == 0 {
            Self(0)
        } else {
            Self((1u64 << len) - 1)
        }
    }

    pub fn bits(self) -> u64 {
        self.0
    }

    pub fn approves(&self, index: usize) -> bool {
        index < 64 && ((self.0 >> index) & 1) == 1
    }

    pub fn approves_all(&self, len: usize) -> bool {
        len <= 64 && self.0 == Self::all_for(len).0
    }
}

fn effective_approval_mask(
    auto_approved_mask: u64,
    manual_approval_mask: u64,
    user_approval_mask: u64,
) -> ApprovalMask {
    ApprovalMask::from_bits(auto_approved_mask | (user_approval_mask & manual_approval_mask))
}

fn pending_approval_from_runtime(runtime: &AgentRuntime) -> Option<PendingApproval> {
    if runtime.status != AgentStatus::WaitingApproval {
        return None;
    }
    let pending = runtime.pending_tool_batch.as_ref()?;
    Some(PendingApproval {
        request_uuid: pending.request_uuid.clone(),
        description: "model requested tool execution".to_string(),
        tool_count: pending.tool_calls.len(),
        auto_approved_mask: pending.auto_approved_mask,
        manual_approval_mask: pending.manual_approval_mask,
    })
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

    #[test]
    fn forget_workspace_removes_only_its_cached_agents() {
        let (_tx, rx) = mpsc::channel(1);
        let mut actor = AgentActor::load(rx, crate::handles::test_handles());
        actor
            .agent_workspace
            .insert("agent-a".to_string(), "workspace-a".to_string());
        actor
            .agent_workspace
            .insert("agent-b".to_string(), "workspace-b".to_string());
        actor
            .runtimes
            .insert("agent-a".to_string(), AgentRuntime::idle());
        actor
            .runtimes
            .insert("agent-b".to_string(), AgentRuntime::idle());

        actor.forget_workspace("workspace-a").unwrap();

        assert!(!actor.agent_workspace.contains_key("agent-a"));
        assert!(!actor.runtimes.contains_key("agent-a"));
        assert_eq!(
            actor.agent_workspace.get("agent-b").map(String::as_str),
            Some("workspace-b")
        );
        assert!(actor.runtimes.contains_key("agent-b"));
    }
}
