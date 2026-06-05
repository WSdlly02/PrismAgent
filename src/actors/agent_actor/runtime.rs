use crate::actors::agent_actor::model::{
    AGENT_ACTOR, AgentActor, AgentEvent, AgentHandle, AgentInferenceOutput, AgentMsg, AgentRuntime,
    AgentSnapshot, AgentStatus, AgentSummary, ApproveRequest, PendingApproval, PendingToolBatch,
    SendMessageRequest, ToolBatchOutput,
};
use crate::actors::agent_actor::pipeline::{
    clone_tool_calls, run_llm_continuation, run_llm_inference, run_tool_batch,
    tool_batch_is_auto_approved, tool_response_units,
};
use crate::actors::context_actor::model::RenderInitialPromptsRequest;
use crate::actors::storage_actor::model::agent::{Agent, AgentCreateRequest};
use crate::actors::storage_actor::model::unit::{Unit, UnitVisibility};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use genai::chat::ToolCall;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

const AUTO_LOOP_MSG: &str = "Go ahead until you have completed all tasks, or use prismagent_finish_task tool to end the loop.";

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
            match msg {
                AgentMsg::List {
                    workspace_uuid,
                    reply,
                } => {
                    let _ = reply.send(self.list(&workspace_uuid).await);
                }
                AgentMsg::Create { request, reply } => {
                    let _ = reply.send(self.create(request).await);
                }
                AgentMsg::Contains {
                    workspace_uuid,
                    agent_uuid,
                    reply,
                } => {
                    let _ = reply.send(Ok(self.contains(&workspace_uuid, &agent_uuid)));
                }
                AgentMsg::Snapshot { agent_uuid, reply } => {
                    let _ = reply.send(self.snapshot(&agent_uuid).await);
                }
                AgentMsg::SendMessage { request, reply } => {
                    let _ = reply.send(self.send_message(request).await);
                }
                AgentMsg::ApproveRequest { request, reply } => {
                    let _ = reply.send(self.approve_request(request).await);
                }
                AgentMsg::Cancel { agent_uuid, reply } => {
                    let _ = reply.send(self.cancel(&agent_uuid).await);
                }
                AgentMsg::SetAutoLoop {
                    agent_uuid,
                    enabled,
                    reply,
                } => {
                    let _ = reply.send(self.set_auto_loop(&agent_uuid, enabled).await);
                }
                AgentMsg::InferenceFinished {
                    agent_uuid,
                    inference_uuid,
                    result,
                } => {
                    self.finish_inference(&agent_uuid, &inference_uuid, result)
                        .await;
                }
                AgentMsg::ToolBatchFinished {
                    agent_uuid,
                    job_uuid,
                    result,
                } => {
                    self.finish_tool_batch(&agent_uuid, &job_uuid, result).await;
                }
            }
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
            self.runtimes.insert(
                agent.uuid.clone(),
                AgentRuntime {
                    status: AgentStatus::Idle,
                    inference_uuid: None,
                    pending_tool_batch: None,
                    active_tool_batch: None,
                },
            );
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
            .map(|agent| AgentSummary {
                agent_uuid: agent.uuid.clone(),
                agent_name: agent.name.clone(),
            })
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
        let initial_units = self
            .handles
            .context
            .render_initial_prompts(RenderInitialPromptsRequest {
                workspace_uuid: workspace_uuid.clone(),
                context_refs,
                profile,
            })
            .await?;
        let mut agent = self
            .handles
            .storage
            .create_agent(request, auto_loop)
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
        self.runtimes.insert(
            agent.uuid.clone(),
            AgentRuntime {
                status: AgentStatus::Idle,
                inference_uuid: None,
                pending_tool_batch: None,
                active_tool_batch: None,
            },
        );
        self.agents.insert(agent.uuid.clone(), agent.clone());
        if has_initial_task && auto_loop {
            self.spawn_llm_continuation(&agent.uuid).await?;
        }
        Ok(agent)
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
        Ok(AgentSnapshot {
            units,
            status: self.runtime(agent_uuid)?.status.clone(),
        })
    }

    async fn send_message(&mut self, request: SendMessageRequest) -> SubsystemResult<()> {
        if self.runtime(&request.agent_uuid)?.status != AgentStatus::Idle {
            return Err(SubsystemError::Conflict {
                resource: "agent_runtime",
                id: request.agent_uuid,
            });
        }
        let agent = self.agent(&request.agent_uuid)?;
        let unit_uuids = agent.unit_chain.clone();
        let profile_name = agent.profile.clone();
        let agent_uuid = request.agent_uuid.clone();
        let workspace_uuid = self.workspace_uuid(&agent_uuid)?.to_string();
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
                .inference_finished(task_agent_uuid, task_inference_uuid, result)
                .await;
        });
        let runtime = self.runtime_mut(&agent_uuid)?;
        runtime.inference_uuid = Some(inference_uuid);
        Ok(())
    }

    async fn finish_inference(
        &mut self,
        agent_uuid: &str,
        inference_uuid: &str,
        result: SubsystemResult<AgentInferenceOutput>,
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
                match self.commit_units(agent_uuid, output.units).await {
                    Ok(()) if is_tool_calls => {
                        if let Err(error) = self.handle_tool_calls(agent_uuid, tool_calls).await {
                            self.emit_event(
                                agent_uuid,
                                AgentEvent::Error {
                                    message: error.to_string(),
                                },
                            );
                            let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                        }
                        return;
                    }
                    Ok(()) => {
                        if self.agent(agent_uuid).is_ok_and(|agent| agent.auto_loop) {
                            if let Err(error) = self.spawn_auto_loop_continuation(agent_uuid).await
                            {
                                self.emit_event(
                                    agent_uuid,
                                    AgentEvent::Error {
                                        message: error.to_string(),
                                    },
                                );
                                let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                            }
                            return;
                        }
                    }
                    Err(error) => {
                        self.emit_event(
                            agent_uuid,
                            AgentEvent::Error {
                                message: error.to_string(),
                            },
                        );
                    }
                }
            }
            Err(error) => {
                self.emit_event(
                    agent_uuid,
                    AgentEvent::Error {
                        message: error.to_string(),
                    },
                );
            }
        }
        let _ = self.set_status(agent_uuid, AgentStatus::Idle);
    }

    async fn finish_tool_batch(
        &mut self,
        agent_uuid: &str,
        job_uuid: &str,
        result: SubsystemResult<ToolBatchOutput>,
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
                if let Err(error) = self.commit_units(agent_uuid, output.units).await {
                    self.emit_event(
                        agent_uuid,
                        AgentEvent::Error {
                            message: error.to_string(),
                        },
                    );
                    let _ = self.set_status(agent_uuid, AgentStatus::Idle);
                    return;
                }
                if continue_loop {
                    if let Err(error) = self.spawn_llm_continuation(agent_uuid).await {
                        self.emit_event(
                            agent_uuid,
                            AgentEvent::Error {
                                message: error.to_string(),
                            },
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
                        &format!("tool batch failed: {error}"),
                    );
                    if let Err(commit_error) = self.commit_units(agent_uuid, units).await {
                        self.emit_event(
                            agent_uuid,
                            AgentEvent::Error {
                                message: commit_error.to_string(),
                            },
                        );
                    }
                }
                self.emit_event(
                    agent_uuid,
                    AgentEvent::Error {
                        message: error.to_string(),
                    },
                );
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
            self.emit_event(agent_uuid, AgentEvent::UnitAppend { unit });
        }
        Ok(())
    }

    async fn approve_request(&mut self, request: ApproveRequest) -> SubsystemResult<()> {
        let pending = {
            let runtime = self.runtime_mut(&request.agent_uuid)?;
            let Some(pending) = runtime.pending_tool_batch.take() else {
                return Err(SubsystemError::InvalidInput {
                    message: format!("no pending approval request: {}", request.request_uuid),
                });
            };
            if pending.request_uuid != request.request_uuid {
                runtime.pending_tool_batch = Some(pending);
                return Err(SubsystemError::InvalidInput {
                    message: format!("unknown approval request: {}", request.request_uuid),
                });
            }
            pending
        };
        self.spawn_tool_batch(
            request.agent_uuid,
            pending.tool_calls,
            approval_mask_from_request(request.approved, request.approved_indices),
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
                        ApprovalMask::None,
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
                        ApprovalMask::None,
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
            return Err(SubsystemError::invalid_input(
                "set_auto_loop(true) is not supported yet",
            ));
        }
        let workspace_uuid = self.workspace_uuid(agent_uuid)?.to_string();
        let agent = self
            .handles
            .storage
            .set_agent_auto_loop(workspace_uuid, agent_uuid.to_string(), enabled)
            .await?;
        self.agents.insert(agent_uuid.to_string(), agent.clone());
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
        let profile_name = self.agent(agent_uuid)?.profile.clone();
        let tools_config = self.handles.profile.tools(profile_name).await?;
        if tool_batch_is_auto_approved(&tools_config, &tool_calls) {
            self.spawn_tool_batch(
                agent_uuid.to_string(),
                tool_calls,
                ApprovalMask::All,
                "tool execution was auto-approved",
            )
            .await
        } else {
            let request_uuid = Uuid::now_v7().to_string();
            self.runtime_mut(agent_uuid)?.pending_tool_batch = Some(PendingToolBatch {
                request_uuid: request_uuid.clone(),
                tool_calls,
            });
            self.set_status(agent_uuid, AgentStatus::WaitingApproval)?;
            self.emit_event(
                agent_uuid,
                AgentEvent::ApproveRequest {
                    request: PendingApproval {
                        request_uuid,
                        description: "model requested tool execution".to_string(),
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
        self.runtime_mut(&agent_uuid)?.active_tool_batch = Some(PendingToolBatch {
            request_uuid: job_uuid.clone(),
            tool_calls: tool_calls.clone(),
        });
        self.set_status(&agent_uuid, AgentStatus::RunningTool)?;
        let handles = self.handles.clone();
        let denied_reason = denied_reason.to_string();
        tokio::spawn(async move {
            let result = run_tool_batch(
                &handles,
                workspace_uuid,
                agent_uuid.clone(),
                profile_name,
                job_uuid.clone(),
                tool_calls,
                approval_mask,
                denied_reason,
            )
            .await;
            let _ = handles
                .agent
                .tool_batch_finished(agent_uuid, job_uuid, result)
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
                .inference_finished(task_agent_uuid, task_inference_uuid, result)
                .await;
        });
        Ok(())
    }

    async fn spawn_auto_loop_continuation(&mut self, agent_uuid: &str) -> SubsystemResult<()> {
        let mut unit = Unit::from_chat_message(genai::chat::ChatMessage::user(AUTO_LOOP_MSG));
        unit.visibility = UnitVisibility::Internal;
        self.commit_units(agent_uuid, vec![unit]).await?;
        self.spawn_llm_continuation(agent_uuid).await
    }

    fn set_status(&mut self, agent_uuid: &str, status: AgentStatus) -> SubsystemResult<()> {
        self.runtime_mut(agent_uuid)?.status = status.clone();
        self.emit_event(agent_uuid, AgentEvent::StatusChanged { status });
        Ok(())
    }

    fn emit_event(&self, agent_uuid: &str, event: AgentEvent) {
        let _ = self
            .handles
            .shell
            .emit_agent_event(agent_uuid.to_string(), event);
    }

    fn agent(&self, agent_uuid: &str) -> SubsystemResult<&Agent> {
        self.agents
            .get(agent_uuid)
            .ok_or_else(|| SubsystemError::not_found("agent", agent_uuid))
    }

    fn runtime(&self, agent_uuid: &str) -> SubsystemResult<&AgentRuntime> {
        self.runtimes
            .get(agent_uuid)
            .ok_or_else(|| SubsystemError::not_found("agent_runtime", agent_uuid))
    }

    fn runtime_mut(&mut self, agent_uuid: &str) -> SubsystemResult<&mut AgentRuntime> {
        self.runtimes
            .get_mut(agent_uuid)
            .ok_or_else(|| SubsystemError::not_found("agent_runtime", agent_uuid))
    }

    fn workspace_uuid(&self, agent_uuid: &str) -> SubsystemResult<&str> {
        self.agent_workspace
            .get(agent_uuid)
            .map(String::as_str)
            .ok_or_else(|| SubsystemError::not_found("agent_workspace", agent_uuid))
    }
}

impl AgentHandle {
    pub async fn list(
        &self,
        workspace_uuid: impl Into<String>,
    ) -> SubsystemResult<Vec<AgentSummary>> {
        request(&self.tx, |reply| AgentMsg::List {
            workspace_uuid: workspace_uuid.into(),
            reply,
        })
        .await
    }

    pub async fn create(&self, request_body: AgentCreateRequest) -> SubsystemResult<Agent> {
        request(&self.tx, |reply| AgentMsg::Create {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn contains(
        &self,
        workspace_uuid: impl Into<String>,
        agent_uuid: impl Into<String>,
    ) -> SubsystemResult<bool> {
        request(&self.tx, |reply| AgentMsg::Contains {
            workspace_uuid: workspace_uuid.into(),
            agent_uuid: agent_uuid.into(),
            reply,
        })
        .await
    }

    pub async fn snapshot(&self, agent_uuid: impl Into<String>) -> SubsystemResult<AgentSnapshot> {
        request(&self.tx, |reply| AgentMsg::Snapshot {
            agent_uuid: agent_uuid.into(),
            reply,
        })
        .await
    }

    pub async fn send_message(&self, request_body: SendMessageRequest) -> SubsystemResult<()> {
        request(&self.tx, |reply| AgentMsg::SendMessage {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn approve_request(&self, request_body: ApproveRequest) -> SubsystemResult<()> {
        request(&self.tx, |reply| AgentMsg::ApproveRequest {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn cancel(&self, agent_uuid: impl Into<String>) -> SubsystemResult<()> {
        request(&self.tx, |reply| AgentMsg::Cancel {
            agent_uuid: agent_uuid.into(),
            reply,
        })
        .await
    }

    pub async fn set_auto_loop(
        &self,
        agent_uuid: impl Into<String>,
        enabled: bool,
    ) -> SubsystemResult<Agent> {
        request(&self.tx, |reply| AgentMsg::SetAutoLoop {
            agent_uuid: agent_uuid.into(),
            enabled,
            reply,
        })
        .await
    }

    pub async fn inference_finished(
        &self,
        agent_uuid: impl Into<String>,
        inference_uuid: impl Into<String>,
        result: SubsystemResult<AgentInferenceOutput>,
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

    pub async fn tool_batch_finished(
        &self,
        agent_uuid: impl Into<String>,
        job_uuid: impl Into<String>,
        result: SubsystemResult<ToolBatchOutput>,
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

#[derive(Clone)]
pub enum ApprovalMask {
    All,
    None,
    Selected(std::collections::HashSet<usize>),
}

impl ApprovalMask {
    pub fn approves(&self, index: usize) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Selected(indices) => indices.contains(&index),
        }
    }

    pub fn approves_all(&self, len: usize) -> bool {
        match self {
            Self::All => true,
            Self::None => false,
            Self::Selected(indices) => {
                indices.len() == len && (0..len).all(|idx| indices.contains(&idx))
            }
        }
    }
}

fn approval_mask_from_request(
    approved: bool,
    approved_indices: Option<Vec<usize>>,
) -> ApprovalMask {
    match approved_indices {
        Some(indices) => ApprovalMask::Selected(indices.into_iter().collect()),
        None if approved => ApprovalMask::All,
        None => ApprovalMask::None,
    }
}

async fn request<T>(
    tx: &mpsc::Sender<AgentMsg>,
    message: impl FnOnce(tokio::sync::oneshot::Sender<SubsystemResult<T>>) -> AgentMsg,
) -> SubsystemResult<T> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(message(reply_tx))
        .await
        .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?;
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(AGENT_ACTOR))?
}
