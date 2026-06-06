use crate::actors::agent_actor::model::{MessageBody, SendMessageRequest};
use crate::actors::storage_actor::model::agent::AgentCreateRequest;
use crate::actors::storage_actor::model::context::Context;
use crate::actors::workflow_actor::model::{
    ContextStatus, ShowMyselfRequest, ShowMyselfResponse, TaskFinishedRequest,
    TaskFinishedResponse, WORKFLOW_ACTOR, WorkflowActor, WorkflowHandle, WorkflowMsg,
    WorkflowRunRequest, WorkflowRuntime, WorkflowTrigger, WorkflowTriggerCreateRequest,
};
use crate::error::{SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::id::petname_uuid;
use std::collections::HashSet;
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
                WorkflowMsg::UuidNew { count, reply } => {
                    let _ = reply.send(uuid_new(count));
                }
                WorkflowMsg::WorkflowNew { request, reply } => {
                    let _ = reply.send(self.handles.storage.create_workflow(request).await);
                }
                WorkflowMsg::WorkflowRun { request, reply } => {
                    let _ = reply.send(self.workflow_run(request).await);
                }
                WorkflowMsg::ContextNew { request, reply } => {
                    let _ = reply.send(self.context_new(request).await);
                }
                WorkflowMsg::TriggerNew { request, reply } => {
                    let _ = reply.send(self.trigger_new(request).await);
                }
                WorkflowMsg::TaskFinished { request, reply } => {
                    let _ = reply.send(self.task_finished(request).await);
                }
                WorkflowMsg::ShowMyself { request, reply } => {
                    let _ = reply.send(self.show_myself(request).await);
                }
            }
        }
    }

    async fn workflow_run(
        &mut self,
        request: WorkflowRunRequest,
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
        Ok(runtime)
    }

    async fn context_new(
        &mut self,
        request: crate::actors::storage_actor::model::context::ContextCreateRequest,
    ) -> SubsystemResult<Context> {
        let context = self.handles.storage.create_context(request).await?;
        self.check_context_triggers(&context.uuid).await;
        Ok(context)
    }

    async fn trigger_new(
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

    async fn task_finished(
        &mut self,
        request: TaskFinishedRequest,
    ) -> SubsystemResult<TaskFinishedResponse> {
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
        let context_out = context_status(
            &self.handles,
            &request.workspace_uuid,
            agent.context_out.clone(),
        )
        .await?;
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
        Ok(TaskFinishedResponse {
            agent_uuid: agent.uuid,
            auto_loop: agent.auto_loop,
            summary: request.summary,
            context_outputs: request.context_outputs,
        })
    }

    async fn show_myself(&self, request: ShowMyselfRequest) -> SubsystemResult<ShowMyselfResponse> {
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
        Ok(ShowMyselfResponse {
            agent_uuid: agent.uuid.clone(),
            name: agent.name.clone(),
            profile: agent.profile.clone(),
            auto_loop: agent.auto_loop,
            context_refs: context_status(
                &self.handles,
                &request.workspace_uuid,
                agent.context_refs.clone(),
            )
            .await?,
            context_out: context_status(
                &self.handles,
                &request.workspace_uuid,
                agent.context_out.clone(),
            )
            .await?,
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
        let delivered = self
            .handles
            .agent
            .send_message(SendMessageRequest {
                agent_uuid,
                message_body: MessageBody {
                    text: format!(
                        "Workflow trigger fired for context {context_uuid}.\n\n{message}"
                    ),
                    attachments: Vec::new(),
                },
            })
            .await
            .is_ok();
        if delivered && let Some(trigger) = self.triggers.get_mut(trigger_uuid) {
            trigger.fired_context_uuids.insert(context_uuid.to_string());
        }
    }
}

impl WorkflowHandle {
    pub async fn uuid_new(&self, count: usize) -> SubsystemResult<Vec<String>> {
        request(&self.tx, |reply| WorkflowMsg::UuidNew { count, reply }).await
    }

    pub async fn workflow_new(
        &self,
        request_body: crate::actors::storage_actor::model::workflow::WorkflowCreateRequest,
    ) -> SubsystemResult<crate::actors::storage_actor::model::workflow::Workflow> {
        request(&self.tx, |reply| WorkflowMsg::WorkflowNew {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn workflow_run(
        &self,
        request_body: WorkflowRunRequest,
    ) -> SubsystemResult<WorkflowRuntime> {
        request(&self.tx, |reply| WorkflowMsg::WorkflowRun {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn context_new(
        &self,
        request_body: crate::actors::storage_actor::model::context::ContextCreateRequest,
    ) -> SubsystemResult<Context> {
        request(&self.tx, |reply| WorkflowMsg::ContextNew {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn trigger_new(
        &self,
        request_body: WorkflowTriggerCreateRequest,
    ) -> SubsystemResult<WorkflowTrigger> {
        request(&self.tx, |reply| WorkflowMsg::TriggerNew {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn task_finished(
        &self,
        request_body: TaskFinishedRequest,
    ) -> SubsystemResult<TaskFinishedResponse> {
        request(&self.tx, |reply| WorkflowMsg::TaskFinished {
            request: request_body,
            reply,
        })
        .await
    }

    pub async fn show_myself(
        &self,
        request_body: ShowMyselfRequest,
    ) -> SubsystemResult<ShowMyselfResponse> {
        request(&self.tx, |reply| WorkflowMsg::ShowMyself {
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

fn uuid_new(count: usize) -> SubsystemResult<Vec<String>> {
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

async fn context_status(
    handles: &AppHandles,
    workspace_uuid: &str,
    context_uuids: Vec<String>,
) -> SubsystemResult<Vec<ContextStatus>> {
    let existing = handles
        .storage
        .list_contexts(workspace_uuid.to_string())
        .await?
        .into_iter()
        .collect::<HashSet<_>>();
    Ok(context_uuids
        .into_iter()
        .map(|context_uuid| ContextStatus {
            exists: existing.contains(&context_uuid),
            context_uuid,
        })
        .collect())
}
