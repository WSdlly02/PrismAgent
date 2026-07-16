use crate::actor_dispatch;
use crate::actors::agent_actor::model::AgentHandle;
use crate::actors::agent_actor::model::{
    AgentSummary, MessageBody, SelfUpdateRequest, SendMessageRequest,
};
use crate::actors::shell_actor::model::WsEvent;
use crate::actors::storage_actor::model::agent::AgentCreateRequest;
use crate::actors::storage_actor::model::context::{Context, ContextCreateRequest};
use crate::actors::storage_actor::model::workflow::{Workflow, WorkflowCreateRequest};
use crate::actors::workflow_actor::dag::{
    WorkflowRuntime, WorkflowSpec, parse_workflow_spec, require_registered, unique_agents,
    unique_contexts, unique_steps, validate_context_flow, validate_runtime_id, validate_step_graph,
};
use crate::actors::workflow_actor::model::{
    AgentView, ContextStatus, ListAgentsRequest, ListAgentsResponse, SelfShowRequest,
    SelfShowResponse, TaskFinishRequest, TaskFinishResponse, WORKFLOW_ACTOR, WorkflowActor,
    WorkflowCancelRequest, WorkflowCancelResponse, WorkflowHandle, WorkflowMsg,
    WorkflowStartRequest,
};
use crate::error::{ResourceKind, SubsystemError, SubsystemResult};
use crate::handles::AppHandles;
use crate::id::petname_uuid;
use crate::impl_handle_methods;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc;

impl WorkflowActor {
    pub fn load(rx: mpsc::Receiver<WorkflowMsg>, handles: AppHandles) -> Self {
        Self {
            rx,
            handles,
            runtimes: std::collections::HashMap::new(),
        }
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.run())
    }

    pub async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            actor_dispatch!(msg;
                WorkflowMsg::UuidGenerate { count ; reply } => uuid_generate(count),
                WorkflowMsg::WorkflowCreate { request ; reply } => self.workflow_create(request).await,
                WorkflowMsg::WorkflowStart { request ; reply } => self.workflow_start(request).await,
                WorkflowMsg::WorkflowCancel { request ; reply } => self.workflow_cancel(request).await,
                WorkflowMsg::ContextCreate { request ; reply } => self.context_create(request).await,
                WorkflowMsg::TaskFinish { request ; reply } => self.task_finish(request).await,
                WorkflowMsg::SelfShow { request ; reply } => self.self_show(request).await,
                WorkflowMsg::ListAgents { request ; reply } => self.list_agents(request).await,
            );
        }
    }

    async fn workflow_start(
        &mut self,
        request: WorkflowStartRequest,
    ) -> SubsystemResult<WorkflowRuntime> {
        validate_runtime_id(&request.workflow_uuid, "workflow uuid")?;
        let workflow = self
            .handles
            .storage
            .read_workflow(
                request.workspace_uuid.clone(),
                request.workflow_uuid.clone(),
            )
            .await?;
        let spec = parse_workflow_spec(&workflow)?;
        self.validate_workflow_spec(&request.workspace_uuid, &workflow, &spec)
            .await?;
        self.sync_planner_context_out(&spec).await?;
        let runtime =
            WorkflowRuntime::new(request.workspace_uuid.clone(), workflow.uuid.clone(), spec);
        let key = (
            runtime.workspace_uuid.clone(),
            runtime.workflow_uuid.clone(),
        );
        self.runtimes.insert(key.clone(), runtime);
        self.advance_workflow(&key).await?;
        let runtime = self
            .runtimes
            .get(&key)
            .cloned()
            .ok_or_else(|| missing_workflow_runtime(&key))?;
        self.emit_workspace_event(
            &runtime.workspace_uuid,
            WsEvent::WorkflowStarted {
                workflow_uuid: runtime.workflow_uuid.clone(),
                planner_agent_uuid: runtime.planner_uuid.clone(),
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
        if !self.runtimes.contains_key(&key) {
            return Err(SubsystemError::not_found(
                ResourceKind::WorkflowRuntime,
                &request.workflow_uuid,
            ));
        }
        // TODO: implement graceful DAG workflow cancellation.
        //
        // Expected semantics:
        // 1. Keep the workflow runtime instead of removing it.
        // 2. Find agents in currently running steps.
        // 3. Update their auto_loop_message so they still create declared context_out
        //    cancellation summaries and call prismagent_task_finish.
        // 4. Cancel current inference/tool work, then send a message to restart cleanup.
        // 5. Let task_finish drive the normal advance/completion path.
        Err(SubsystemError::Unsupported {
            feature: "DAG workflow cancellation",
        })
    }

    async fn workflow_create(
        &mut self,
        request: WorkflowCreateRequest,
    ) -> SubsystemResult<Workflow> {
        let workspace_uuid = request.workspace_uuid.clone();
        let workflow = self.handles.storage.create_workflow(request).await?;
        self.emit_workspace_event(
            &workspace_uuid,
            WsEvent::WorkflowCreated {
                workflow_uuid: workflow.uuid.clone(),
                title: workflow.title.clone(),
            },
        );
        Ok(workflow)
    }

    async fn context_create(&mut self, request: ContextCreateRequest) -> SubsystemResult<Context> {
        let workspace_uuid = request.workspace_uuid.clone();
        let context = self.handles.storage.create_context(request).await?;
        self.emit_workspace_event(
            &workspace_uuid,
            WsEvent::ContextCreated {
                context_uuid: context.uuid.clone(),
                title: context.title.clone(),
            },
        );
        Ok(context)
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
            .ok_or_else(|| SubsystemError::not_found(ResourceKind::Agent, &request.agent_uuid))?;
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
            return Err(SubsystemError::validation(format!(
                "context_out is not complete: {}",
                missing.join(", ")
            )));
        }
        let agent = self
            .handles
            .agent
            .set_auto_loop(request.agent_uuid.clone(), false)
            .await?;
        self.advance_workflows(&request.workspace_uuid).await?;
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
        .ok_or_else(|| SubsystemError::not_found(ResourceKind::Agent, request.agent_uuid))
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

    async fn validate_workflow_spec(
        &self,
        workspace_uuid: &str,
        workflow: &Workflow,
        spec: &WorkflowSpec,
    ) -> SubsystemResult<()> {
        if spec.workflow.uuid != workflow.uuid {
            return Err(SubsystemError::validation(format!(
                "workflow uuid mismatch: outer={}, content={}",
                workflow.uuid, spec.workflow.uuid
            )));
        }
        if spec.workflow.title != workflow.title {
            return Err(SubsystemError::validation(format!(
                "workflow title mismatch: outer={}, content={}",
                workflow.title, spec.workflow.title
            )));
        }
        validate_runtime_id(&spec.workflow.planner_uuid, "planner uuid")?;
        self.handles
            .storage
            .read_agents(
                workspace_uuid.to_string(),
                vec![spec.workflow.planner_uuid.clone()],
            )
            .await?;

        let registered_contexts = unique_contexts(spec)?;
        let existing_contexts = self
            .handles
            .storage
            .list_contexts(workspace_uuid.to_string())
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        if spec.workflow.planner_context_out.is_empty() {
            return Err(SubsystemError::validation(
                "workflow.planner_context_out must not be empty",
            ));
        }
        for context_uuid in &spec.workflow.planner_context_out {
            require_registered(&registered_contexts, "planner_context_out", context_uuid)?;
            if !existing_contexts.contains(context_uuid) {
                return Err(SubsystemError::not_found(
                    ResourceKind::Context,
                    context_uuid,
                ));
            }
        }
        if spec.workflow.final_piped_contexts.is_empty() {
            return Err(SubsystemError::validation(
                "workflow.final_piped_contexts must not be empty",
            ));
        }
        for context_uuid in &spec.workflow.final_piped_contexts {
            require_registered(&registered_contexts, "final_piped_contexts", context_uuid)?;
        }

        let profiles = self
            .handles
            .profile
            .list_profiles()
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        let agents = unique_agents(spec, &registered_contexts, &profiles)?;
        let steps = unique_steps(spec, &agents)?;
        validate_step_graph(&steps)?;
        validate_context_flow(spec, &agents, &steps)?;

        let produced_contexts = agents
            .values()
            .flat_map(|agent| agent.context_out.iter().cloned())
            .chain(spec.workflow.planner_context_out.iter().cloned())
            .collect::<HashSet<_>>();
        for context_uuid in &spec.workflow.final_piped_contexts {
            if !produced_contexts.contains(context_uuid) {
                return Err(SubsystemError::validation(format!(
                    "final_piped_contexts references unproduced context: {context_uuid}"
                )));
            }
        }
        Ok(())
    }

    async fn sync_planner_context_out(&self, spec: &WorkflowSpec) -> SubsystemResult<()> {
        self.handles
            .agent
            .self_update(SelfUpdateRequest {
                agent_uuid: spec.workflow.planner_uuid.clone(),
                context_refs: None,
                context_out: Some(spec.workflow.planner_context_out.clone()),
                auto_loop: None,
                auto_loop_message: None,
            })
            .await?;
        Ok(())
    }

    async fn advance_workflows(&mut self, workspace_uuid: &str) -> SubsystemResult<()> {
        let keys = self
            .runtimes
            .keys()
            .filter(|(runtime_workspace_uuid, _)| runtime_workspace_uuid == workspace_uuid)
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.advance_workflow(&key).await?;
        }
        Ok(())
    }

    async fn advance_workflow(&mut self, key: &(String, String)) -> SubsystemResult<()> {
        loop {
            let existing_contexts = {
                let workspace_uuid = &key.0;
                self.handles
                    .storage
                    .list_contexts(workspace_uuid.clone())
                    .await?
                    .into_iter()
                    .collect::<HashSet<_>>()
            };

            let runtime = self
                .runtimes
                .get_mut(key)
                .ok_or_else(|| missing_workflow_runtime(key))?;
            runtime.mark_completed_steps(&existing_contexts);
            let ready_steps = runtime.ready_step_ids();

            if ready_steps.is_empty() {
                break;
            }
            for step_id in ready_steps {
                self.start_agent_step(key, &step_id).await?;
            }
        }

        let should_complete = {
            let runtime = self
                .runtimes
                .get(key)
                .ok_or_else(|| missing_workflow_runtime(key))?;
            !runtime.completed && runtime.all_steps_done()
        };
        if should_complete {
            self.complete_workflow(key).await?;
        }
        Ok(())
    }

    async fn start_agent_step(
        &mut self,
        key: &(String, String),
        step_id: &str,
    ) -> SubsystemResult<()> {
        let (workspace_uuid, agents) = {
            let runtime = self
                .runtimes
                .get(key)
                .ok_or_else(|| missing_workflow_runtime(key))?;
            let agents = runtime.step_agents(step_id)?;
            (runtime.workspace_uuid.clone(), agents)
        };

        for agent in agents {
            self.handles
                .agent
                .create(AgentCreateRequest {
                    workspace_uuid: workspace_uuid.clone(),
                    uuid: agent.uuid,
                    name: agent.name,
                    profile: agent.profile,
                    context_refs: agent.context_refs,
                    context_out: agent.context_out,
                })
                .await?;
        }
        self.runtimes
            .get_mut(key)
            .ok_or_else(|| missing_workflow_runtime(key))?
            .mark_step_running(step_id)?;
        Ok(())
    }

    async fn complete_workflow(&mut self, key: &(String, String)) -> SubsystemResult<()> {
        let (workspace_uuid, workflow_uuid, planner_uuid, final_contexts) = {
            let runtime = self
                .runtimes
                .get(key)
                .ok_or_else(|| missing_workflow_runtime(key))?;
            (
                runtime.workspace_uuid.clone(),
                runtime.workflow_uuid.clone(),
                runtime.planner_uuid.clone(),
                runtime.spec.workflow.final_piped_contexts.clone(),
            )
        };
        let message = self
            .render_final_piped_message(&workspace_uuid, &workflow_uuid, final_contexts)
            .await?;
        let agent = self.handles.agent.clone();
        tokio::spawn(async move { deliver_message_with_retry(agent, planner_uuid, message).await });
        self.runtimes
            .get_mut(key)
            .ok_or_else(|| missing_workflow_runtime(key))?
            .mark_completed();
        Ok(())
    }

    async fn render_final_piped_message(
        &self,
        workspace_uuid: &str,
        workflow_uuid: &str,
        context_uuids: Vec<String>,
    ) -> SubsystemResult<String> {
        let contexts = self
            .handles
            .storage
            .read_contexts(workspace_uuid.to_string(), context_uuids)
            .await?;
        let mut message = format!(
            "# Workflow Completed\n\nWorkflow `{workflow_uuid}` completed. Final contexts are piped below.\n"
        );
        for context in contexts {
            message.push_str(&format!(
                "\n\n## Context `{}`: {}\n\n{}",
                context.uuid, context.title, context.content
            ));
        }
        Ok(message)
    }

    fn emit_workspace_event(&self, workspace_uuid: &str, event: WsEvent) {
        let _ = self
            .handles
            .shell
            .emit_workspace_event(workspace_uuid.to_string(), event);
    }
}

async fn deliver_message_with_retry(agent: AgentHandle, agent_uuid: String, message: String) {
    let mut delay = Duration::from_secs(1);
    loop {
        let result = agent
            .send_message(SendMessageRequest {
                agent_uuid: agent_uuid.clone(),
                message_body: MessageBody {
                    text: message.clone(),
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
                    "Failed to deliver complete message to agent {agent_uuid}\nretrying in {delay:?}... (error: {result:?})"
                );
            }
            Err(_) => return,
        }
    }
}
// ---- Handle methods (macro-generated) ----

impl_handle_methods! {
    WorkflowHandle for WorkflowMsg, WORKFLOW_ACTOR;

    fn uuid_generate(&self, count: usize) -> Vec<String>
        => UuidGenerate { count: count };

    fn workflow_create(&self, request: WorkflowCreateRequest) -> Workflow
        => WorkflowCreate { request: request };

    fn workflow_start(&self, request: WorkflowStartRequest) -> WorkflowRuntime
        => WorkflowStart { request: request };

    fn workflow_cancel(&self, request: WorkflowCancelRequest) -> WorkflowCancelResponse
        => WorkflowCancel { request: request };

    fn context_create(&self, request: ContextCreateRequest) -> Context
        => ContextCreate { request: request };

    fn task_finish(&self, request: TaskFinishRequest) -> TaskFinishResponse
        => TaskFinish { request: request };

    fn self_show(&self, request: SelfShowRequest) -> SelfShowResponse
        => SelfShow { request: request };

    fn list_agents(&self, request: ListAgentsRequest) -> ListAgentsResponse
        => ListAgents { request: request };
}

// ---- Free functions ----

fn uuid_generate(count: usize) -> SubsystemResult<Vec<String>> {
    if count == 0 || count > 64 {
        return Err(SubsystemError::validation(
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

fn missing_workflow_runtime(key: &(String, String)) -> SubsystemError {
    SubsystemError::internal(
        "access workflow runtime",
        format!(
            "workflow runtime {} is missing from workspace {}",
            key.1, key.0
        ),
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorClass;

    #[test]
    fn missing_runtime_is_an_internal_invariant_failure() {
        let key = ("workspace-1".to_string(), "workflow-1".to_string());
        let error = missing_workflow_runtime(&key);

        assert_eq!(error.descriptor().class, ErrorClass::Internal);
        assert!(error.public_error().message.contains("workspace-1"));
        assert!(error.public_error().message.contains("workflow-1"));
    }
}
