use crate::actors::agent_actor::model::AgentHandle;
use crate::actors::agent_actor::model::{AgentSummary, MessageBody, SendMessageRequest};
use crate::actors::shell_actor::model::WorkspaceEvent;
use crate::actors::storage_actor::model::agent::AgentCreateRequest;
use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::workflow::Workflow;
use crate::actors::workflow_actor::model::{
    AgentView, ContextStatus, ListAgentsRequest, ListAgentsResponse, SelfShowRequest,
    SelfShowResponse, TaskFinishRequest, TaskFinishResponse, WORKFLOW_ACTOR, WorkflowActor,
    WorkflowCancelRequest, WorkflowCancelResponse, WorkflowHandle, WorkflowMsg, WorkflowRuntime,
    WorkflowStartRequest, WorkflowTrigger, WorkflowTriggerCreateRequest,
};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::id::petname_uuid;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc;

impl WorkflowActor {
    pub fn load(rx: mpsc::Receiver<WorkflowMsg>, handles: AppHandles) -> Self {
        Self {
            rx,
            handles,
            runtimes: std::collections::HashMap::new(),
            triggers: std::collections::HashMap::new(),
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                WorkflowMsg::UuidGenerate { count, reply } => {
                    let _ = reply.send(uuid_generate(count));
                }
                WorkflowMsg::WorkflowCreate { request, reply } => {
                    let _ = reply.send(self.workflow_create(request).await);
                }
                WorkflowMsg::WorkflowStart { request, reply } => {
                    let _ = reply.send(self.workflow_start(request).await);
                }
                WorkflowMsg::WorkflowCancel { request, reply } => {
                    let _ = reply.send(self.workflow_cancel(request).await);
                }
                WorkflowMsg::ContextCreate { request, reply } => {
                    let _ = reply.send(self.context_create(request).await);
                }
                WorkflowMsg::TriggerCreate { request, reply } => {
                    let _ = reply.send(self.trigger_create(request).await);
                }
                WorkflowMsg::TaskFinish { request, reply } => {
                    let _ = reply.send(self.task_finish(request).await);
                }
                WorkflowMsg::SelfShow { request, reply } => {
                    let _ = reply.send(self.self_show(request).await);
                }
                WorkflowMsg::ListAgents { request, reply } => {
                    let _ = reply.send(self.list_agents(request).await);
                }
            }
        }
    }

    async fn workflow_start(
        &mut self,
        request: WorkflowStartRequest,
    ) -> SubsystemResult<WorkflowRuntime> {
        let workflow = self
            .handles
            .storage
            .read_workflow(
                request.workspace_uuid.clone(),
                request.workflow_uuid.clone(),
            )
            .await?;
        let agent = self
            .handles
            .agent
            .create(AgentCreateRequest {
                workspace_uuid: request.workspace_uuid.clone(),
                uuid: request.coordinator_uuid.clone(),
                name: request.coordinator_name,
                profile: request.coordinator_profile,
                context_refs: Vec::new(),
                context_out: Vec::new(),
            })
            .await?;
        let runtime = WorkflowRuntime {
            workspace_uuid: request.workspace_uuid.clone(),
            workflow_uuid: workflow.uuid.clone(),
            coordinator_agent_uuid: agent.uuid.clone(),
        };
        self.runtimes.insert(
            (
                runtime.workspace_uuid.clone(),
                runtime.workflow_uuid.clone(),
            ),
            runtime.clone(),
        );
        self.handles
            .agent
            .send_message(SendMessageRequest {
                agent_uuid: agent.uuid,
                message_body: MessageBody {
                    text: format!(
                        "# Workflow {}\n\n{}\n\nStart coordinating this workflow. Create agents, contexts, and triggers as needed.",
                        workflow.uuid, workflow.content
                    ),
                    attachments: Vec::new(),
                },
            })
            .await?;
        self.emit_workspace_event(
            &runtime.workspace_uuid,
            WorkspaceEvent::WorkflowStarted {
                workflow_uuid: runtime.workflow_uuid.clone(),
                coordinator_agent_uuid: runtime.coordinator_agent_uuid.clone(),
            },
        );
        Ok(runtime)
    }

    async fn workflow_cancel(
        &mut self,
        request: WorkflowCancelRequest,
    ) -> SubsystemResult<WorkflowCancelResponse> {
        let key = (
            request.workspace_uuid.clone(),
            request.workflow_uuid.clone(),
        );
        let runtime =
            self.runtimes.get(&key).cloned().ok_or_else(|| {
                SubsystemError::not_found("workflow_runtime", &request.workflow_uuid)
            })?;
        self.handles
            .agent
            .send_message(SendMessageRequest {
                agent_uuid: runtime.coordinator_agent_uuid.clone(),
                message_body: MessageBody {
                    text: format!(
                        "# Workflow Cancellation\n\nWorkflow `{}` has been cancelled by the user.\n\nReason:\n{}\n\nCoordinate a clean cancellation:\n1. Use prismagent_agent_list to inspect related agents.\n2. Use prismagent_agent_terminate for agents that should stop cleanly.\n3. Ensure required context_out documents are written if cleanup requires them.\n4. Let workflow triggers fire normally as cleanup contexts are produced.\n5. When coordinator cleanup is complete, wait for the next trigger or report completion as appropriate.",
                        runtime.workflow_uuid,
                        request.reason.trim()
                    ),
                    attachments: Vec::new(),
                },
            })
            .await?;
        self.emit_workspace_event(
            &runtime.workspace_uuid,
            WorkspaceEvent::WorkflowCancelRequested {
                workflow_uuid: runtime.workflow_uuid.clone(),
                coordinator_agent_uuid: runtime.coordinator_agent_uuid.clone(),
            },
        );
        Ok(WorkflowCancelResponse {
            workspace_uuid: runtime.workspace_uuid,
            workflow_uuid: runtime.workflow_uuid,
            coordinator_agent_uuid: runtime.coordinator_agent_uuid,
        })
    }

    async fn workflow_create(
        &mut self,
        request: crate::actors::storage_actor::model::workflow::WorkflowCreateRequest,
    ) -> SubsystemResult<Workflow> {
        let workspace_uuid = request.workspace_uuid.clone();
        let workflow = self.handles.storage.create_workflow(request).await?;
        self.emit_workspace_event(
            &workspace_uuid,
            WorkspaceEvent::WorkflowCreated {
                workflow_uuid: workflow.uuid.clone(),
                title: workflow.title.clone(),
            },
        );
        Ok(workflow)
    }

    async fn context_create(
        &mut self,
        request: crate::actors::storage_actor::model::context::ContextCreateRequest,
    ) -> SubsystemResult<Context> {
        let workspace_uuid = request.workspace_uuid.clone();
        let context = self.handles.storage.create_context(request).await?;
        self.emit_workspace_event(
            &workspace_uuid,
            WorkspaceEvent::ContextCreated {
                context_uuid: context.uuid.clone(),
                title: context.title.clone(),
            },
        );
        self.check_context_triggers(&context.uuid).await;
        Ok(context)
    }

    async fn trigger_create(
        &mut self,
        request: WorkflowTriggerCreateRequest,
    ) -> SubsystemResult<WorkflowTrigger> {
        validate_runtime_id(&request.uuid, "trigger uuid")?;
        validate_runtime_id(&request.workflow_uuid, "workflow uuid")?;
        validate_runtime_id(&request.coordinator_agent_uuid, "coordinator agent uuid")?;
        if request.message.trim().is_empty() {
            return Err(SubsystemError::invalid_input(
                "trigger message must not be empty",
            ));
        }
        if request.context_uuids.is_empty() {
            return Err(SubsystemError::invalid_input(
                "trigger context_uuids must not be empty",
            ));
        }
        for context_uuid in &request.context_uuids {
            validate_runtime_id(context_uuid, "context uuid")?;
        }
        let trigger = WorkflowTrigger {
            uuid: request.uuid,
            workspace_uuid: request.workspace_uuid,
            workflow_uuid: request.workflow_uuid,
            coordinator_agent_uuid: request.coordinator_agent_uuid,
            context_uuids: request.context_uuids,
            fired_context_uuids: HashSet::new(),
            message: request.message,
            enabled: true,
        };
        if self.triggers.contains_key(&trigger.uuid) {
            return Err(SubsystemError::Conflict {
                resource: "workflow_trigger",
                id: trigger.uuid,
            });
        }
        let trigger_uuid = trigger.uuid.clone();
        self.triggers.insert(trigger_uuid.clone(), trigger);
        self.check_existing_trigger_contexts(&trigger_uuid).await?;
        self.triggers
            .get(&trigger_uuid)
            .cloned()
            .ok_or_else(|| SubsystemError::not_found("workflow_trigger", trigger_uuid))
    }

    async fn task_finish(
        &mut self,
        request: TaskFinishRequest,
    ) -> SubsystemResult<TaskFinishResponse> {
        let agents = self
            .handles
            .storage
            .read_agents(
                request.workspace_uuid.clone(),
                vec![request.agent_uuid.clone()],
            )
            .await?;
        let agent = agents
            .first()
            .ok_or_else(|| SubsystemError::not_found("agent", &request.agent_uuid))?;
        let existing_contexts = self
            .handles
            .storage
            .list_contexts(request.workspace_uuid.clone())
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        let context_out =
            context_status_from_existing(agent.context_out.clone(), &existing_contexts);
        let missing = context_out
            .iter()
            .filter(|status| !status.exists)
            .map(|status| status.context_uuid.clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(SubsystemError::invalid_input(format!(
                "context_out is not complete: {}",
                missing.join(", ")
            )));
        }
        let agent = self
            .handles
            .agent
            .set_auto_loop(request.agent_uuid.clone(), false)
            .await?;
        Ok(TaskFinishResponse {
            agent_uuid: agent.uuid,
            auto_loop: agent.auto_loop,
            summary: request.summary,
            context_outputs: request.context_outputs,
        })
    }

    async fn self_show(&self, request: SelfShowRequest) -> SubsystemResult<SelfShowResponse> {
        self.list_agents(ListAgentsRequest {
            workspace_uuid: request.workspace_uuid,
        })
        .await?
        .agents
        .into_iter()
        .find(|agent| agent.agent_uuid == request.agent_uuid)
        .ok_or_else(|| SubsystemError::not_found("agent", request.agent_uuid))
    }

    async fn list_agents(&self, request: ListAgentsRequest) -> SubsystemResult<ListAgentsResponse> {
        let agents = self
            .handles
            .agent
            .list(request.workspace_uuid.clone())
            .await?;
        let existing_contexts = self
            .handles
            .storage
            .list_contexts(request.workspace_uuid)
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        Ok(ListAgentsResponse {
            agents: agents
                .into_iter()
                .map(|agent| agent_view(agent, &existing_contexts))
                .collect(),
        })
    }

    async fn check_existing_trigger_contexts(&mut self, trigger_uuid: &str) -> SubsystemResult<()> {
        let Some(trigger) = self.triggers.get(trigger_uuid).cloned() else {
            return Ok(());
        };
        let existing = self
            .handles
            .storage
            .list_contexts(trigger.workspace_uuid)
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        for context_uuid in trigger.context_uuids {
            if existing.contains(&context_uuid) {
                self.fire_trigger(trigger_uuid, &context_uuid).await;
            }
        }
        Ok(())
    }

    async fn check_context_triggers(&mut self, context_uuid: &str) {
        let trigger_uuids = self
            .triggers
            .iter()
            .filter(|(_, trigger)| {
                trigger.enabled && trigger.context_uuids.iter().any(|id| id == context_uuid)
            })
            .map(|(uuid, _)| uuid.clone())
            .collect::<Vec<_>>();
        for trigger_uuid in trigger_uuids {
            self.fire_trigger(&trigger_uuid, context_uuid).await;
        }
    }

    async fn fire_trigger(&mut self, trigger_uuid: &str, context_uuid: &str) {
        let Some(trigger) = self.triggers.get(trigger_uuid) else {
            return;
        };
        if trigger.fired_context_uuids.contains(context_uuid) {
            return;
        }
        let agent_uuid = trigger.coordinator_agent_uuid.clone();
        let message = trigger
            .message
            .replace("{context_uuid}", context_uuid)
            .replace("{workflow_uuid}", &trigger.workflow_uuid);
        if let Some(trigger) = self.triggers.get_mut(trigger_uuid) {
            trigger.fired_context_uuids.insert(context_uuid.to_string());
        }
        let agent = self.handles.agent.clone();
        let context_uuid = context_uuid.to_string();
        tokio::spawn(async move {
            deliver_trigger_message(agent, agent_uuid, context_uuid, message).await;
        });
    }

    fn emit_workspace_event(&self, workspace_uuid: &str, event: WorkspaceEvent) {
        let _ = self
            .handles
            .shell
            .emit_workspace_event(workspace_uuid.to_string(), event);
    }
}

async fn deliver_trigger_message(
    agent: AgentHandle,
    agent_uuid: String,
    context_uuid: String,
    message: String,
) {
    let mut delay = Duration::from_secs(1);
    loop {
        let result = agent
            .send_message(SendMessageRequest {
                agent_uuid: agent_uuid.clone(),
                message_body: MessageBody {
                    text: format!(
                        "Workflow trigger fired for context {context_uuid}.\n\n{message}\nNotice: The trigger message may be delayed"
                    ),
                    attachments: Vec::new(),
                },
            })
            .await;
        match result {
            Ok(()) => return,
            Err(SubsystemError::Conflict { .. }) | Err(SubsystemError::Timeout { .. }) => {
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(10));
                eprintln!(
                    "Failed to deliver trigger message for context {context_uuid} to agent {agent_uuid}\nretrying in {delay:?}... (error: {result:?})"
                );
            }
            Err(_) => return,
        }
    }
}

impl WorkflowHandle {
    pub async fn uuid_generate(&self, count: usize) -> SubsystemResult<Vec<String>> {
        request(&self.tx, |reply| WorkflowMsg::UuidGenerate { count, reply }).await
    }

    pub async fn workflow_create(
        &self,
        request_body: crate::actors::storage_actor::model::workflow::WorkflowCreateRequest,
    ) -> SubsystemResult<crate::actors::storage_actor::model::workflow::Workflow> {
        request(&self.tx, |reply| WorkflowMsg::WorkflowCreate {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn workflow_start(
        &self,
        request_body: WorkflowStartRequest,
    ) -> SubsystemResult<WorkflowRuntime> {
        request(&self.tx, |reply| WorkflowMsg::WorkflowStart {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn context_create(
        &self,
        request_body: crate::actors::storage_actor::model::context::ContextCreateRequest,
    ) -> SubsystemResult<Context> {
        request(&self.tx, |reply| WorkflowMsg::ContextCreate {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn trigger_create(
        &self,
        request_body: WorkflowTriggerCreateRequest,
    ) -> SubsystemResult<WorkflowTrigger> {
        request(&self.tx, |reply| WorkflowMsg::TriggerCreate {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn task_finish(
        &self,
        request_body: TaskFinishRequest,
    ) -> SubsystemResult<TaskFinishResponse> {
        request(&self.tx, |reply| WorkflowMsg::TaskFinish {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn self_show(
        &self,
        request_body: SelfShowRequest,
    ) -> SubsystemResult<SelfShowResponse> {
        request(&self.tx, |reply| WorkflowMsg::SelfShow {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn list_agents(
        &self,
        request_body: ListAgentsRequest,
    ) -> SubsystemResult<ListAgentsResponse> {
        request(&self.tx, |reply| WorkflowMsg::ListAgents {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn workflow_cancel(
        &self,
        request_body: WorkflowCancelRequest,
    ) -> SubsystemResult<WorkflowCancelResponse> {
        request(&self.tx, |reply| WorkflowMsg::WorkflowCancel {
            request: request_body,
            reply,
        })
        .await
    }
}

fn validate_runtime_id(value: &str, field: &'static str) -> SubsystemResult<()> {
    if !value.trim().is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && value != "."
        && value != ".."
        && !value.ends_with(".json")
    {
        Ok(())
    } else {
        Err(SubsystemError::invalid_input(format!(
            "invalid {field}: {value}"
        )))
    }
}

async fn request<T>(
    tx: &mpsc::Sender<WorkflowMsg>,
    message: impl FnOnce(tokio::sync::oneshot::Sender<SubsystemResult<T>>) -> WorkflowMsg,
) -> SubsystemResult<T> {
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    tx.send(message(reply_tx))
        .await
        .map_err(|_| SubsystemError::actor_dead(WORKFLOW_ACTOR))?;
    reply_rx
        .await
        .map_err(|_| SubsystemError::actor_dead(WORKFLOW_ACTOR))?
}

fn uuid_generate(count: usize) -> SubsystemResult<Vec<String>> {
    if count == 0 || count > 64 {
        return Err(SubsystemError::invalid_input(
            "uuid count must be between 1 and 64",
        ));
    }
    let mut uuids = Vec::with_capacity(count);
    while uuids.len() < count {
        let candidate = petname_uuid(uuids.clone())?;
        if !uuids.contains(&candidate) {
            uuids.push(candidate);
        }
    }
    Ok(uuids)
}

fn agent_view(agent: AgentSummary, existing_contexts: &HashSet<String>) -> AgentView {
    AgentView {
        agent_uuid: agent.agent_uuid,
        name: agent.agent_name,
        profile: agent.profile,
        auto_loop: agent.auto_loop,
        status: agent.status,
        context_refs: context_status_from_existing(agent.context_refs, existing_contexts),
        context_out: context_status_from_existing(agent.context_out, existing_contexts),
    }
}

fn context_status_from_existing(
    context_uuids: Vec<String>,
    existing: &HashSet<String>,
) -> Vec<ContextStatus> {
    context_uuids
        .into_iter()
        .map(|context_uuid| ContextStatus {
            exists: existing.contains(&context_uuid),
            context_uuid,
        })
        .collect()
}
