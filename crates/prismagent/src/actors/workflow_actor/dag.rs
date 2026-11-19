use crate::actors::storage_actor::model::workflow::Workflow;
use crate::error::{SubsystemError, SubsystemResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRuntime {
    pub workspace_uuid: String,
    pub workflow_uuid: String,
    pub planner_uuid: String,
    pub spec: WorkflowSpec,
    pub steps: HashMap<String, WorkflowStepRuntime>,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepRuntime {
    pub status: WorkflowStepStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Pending,
    Running,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSpec {
    pub workflow: WorkflowHeader,
    #[serde(default)]
    pub context: Vec<WorkflowContextSpec>,
    #[serde(default)]
    pub agent: Vec<WorkflowAgentSpec>,
    #[serde(default)]
    pub step: Vec<WorkflowStepSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowHeader {
    pub uuid: String,
    pub title: String,
    pub planner_uuid: String,
    #[serde(default)]
    pub planner_context_out: Vec<String>,
    #[serde(default)]
    pub final_piped_contexts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContextSpec {
    pub uuid: String,
    pub title: String,
    #[serde(default)]
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowAgentSpec {
    pub uuid: String,
    pub profile: String,
    pub name: String,
    #[serde(default)]
    pub context_refs: Vec<String>,
    #[serde(default)]
    pub context_out: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStepSpec {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub agents: Vec<String>,
}

impl WorkflowRuntime {
    pub fn new(workspace_uuid: String, workflow_uuid: String, spec: WorkflowSpec) -> Self {
        let steps = spec
            .step
            .iter()
            .map(|step| {
                (
                    step.id.clone(),
                    WorkflowStepRuntime {
                        status: WorkflowStepStatus::Pending,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        Self {
            workspace_uuid,
            workflow_uuid,
            planner_uuid: spec.workflow.planner_uuid.clone(),
            spec,
            steps,
            completed: false,
        }
    }

    pub fn mark_completed_steps(&mut self, existing_contexts: &HashSet<String>) {
        let newly_done = self
            .spec
            .step
            .iter()
            .filter(|step| {
                self.steps
                    .get(&step.id)
                    .is_some_and(|state| state.status == WorkflowStepStatus::Running)
                    && step_complete(step, &self.spec, existing_contexts)
            })
            .map(|step| step.id.clone())
            .collect::<Vec<_>>();
        for step_id in newly_done {
            if let Some(step_runtime) = self.steps.get_mut(&step_id) {
                step_runtime.status = WorkflowStepStatus::Done;
            }
        }
    }

    pub fn ready_step_ids(&self) -> Vec<String> {
        self.spec
            .step
            .iter()
            .filter(|step| {
                self.steps
                    .get(&step.id)
                    .is_some_and(|state| state.status == WorkflowStepStatus::Pending)
                    && step.needs.iter().all(|need| {
                        self.steps
                            .get(need)
                            .is_some_and(|state| state.status == WorkflowStepStatus::Done)
                    })
            })
            .map(|step| step.id.clone())
            .collect()
    }

    pub fn mark_step_running(&mut self, step_id: &str) -> SubsystemResult<()> {
        let step_runtime = self
            .steps
            .get_mut(step_id)
            .ok_or_else(|| workflow_invariant("mark workflow step running", "step", step_id))?;
        step_runtime.status = WorkflowStepStatus::Running;
        Ok(())
    }

    pub fn all_steps_done(&self) -> bool {
        self.steps
            .values()
            .all(|step| step.status == WorkflowStepStatus::Done)
    }

    pub fn mark_completed(&mut self) {
        self.completed = true;
    }

    pub fn step_agents(&self, step_id: &str) -> SubsystemResult<Vec<WorkflowAgentSpec>> {
        let step = self
            .spec
            .step
            .iter()
            .find(|step| step.id == step_id)
            .ok_or_else(|| workflow_invariant("resolve workflow step agents", "step", step_id))?;
        let agents_by_uuid = self
            .spec
            .agent
            .iter()
            .map(|agent| (agent.uuid.as_str(), agent))
            .collect::<HashMap<_, _>>();
        step.agents
            .iter()
            .map(|agent_uuid| {
                agents_by_uuid
                    .get(agent_uuid.as_str())
                    .map(|agent| (*agent).clone())
                    .ok_or_else(|| {
                        workflow_invariant("resolve workflow step agents", "agent", agent_uuid)
                    })
            })
            .collect()
    }
}

pub fn parse_workflow_spec(workflow: &Workflow) -> SubsystemResult<WorkflowSpec> {
    toml::from_str(&workflow.content)
        .map_err(|error| SubsystemError::validation(format!("invalid workflow TOML: {error}")))
}

pub fn unique_contexts(spec: &WorkflowSpec) -> SubsystemResult<HashSet<String>> {
    let mut contexts = HashSet::new();
    for context in &spec.context {
        validate_runtime_id(&context.uuid, "context uuid")?;
        if context.title.trim().is_empty() {
            return Err(SubsystemError::validation(format!(
                "context title must not be empty: {}",
                context.uuid
            )));
        }
        if !contexts.insert(context.uuid.clone()) {
            return Err(SubsystemError::validation(format!(
                "duplicate context uuid: {}",
                context.uuid
            )));
        }
    }
    Ok(contexts)
}

pub fn unique_agents(
    spec: &WorkflowSpec,
    registered_contexts: &HashSet<String>,
    profiles: &HashSet<String>,
) -> SubsystemResult<HashMap<String, WorkflowAgentSpec>> {
    let mut agents = HashMap::new();
    for agent in &spec.agent {
        validate_runtime_id(&agent.uuid, "agent uuid")?;
        if agent.name.trim().is_empty() {
            return Err(SubsystemError::validation(format!(
                "agent name must not be empty: {}",
                agent.uuid
            )));
        }
        if !profiles.contains(&agent.profile) {
            return Err(SubsystemError::validation(format!(
                "unknown agent profile for {}: {}",
                agent.uuid, agent.profile
            )));
        }
        if agent.context_refs.is_empty() {
            return Err(SubsystemError::validation(format!(
                "agent.context_refs must not be empty: {}",
                agent.uuid
            )));
        }
        if agent.context_out.is_empty() {
            return Err(SubsystemError::validation(format!(
                "agent.context_out must not be empty: {}",
                agent.uuid
            )));
        }
        for context_uuid in agent.context_refs.iter().chain(agent.context_out.iter()) {
            require_registered(registered_contexts, "agent context", context_uuid)?;
        }
        if agents.insert(agent.uuid.clone(), agent.clone()).is_some() {
            return Err(SubsystemError::validation(format!(
                "duplicate agent uuid: {}",
                agent.uuid
            )));
        }
    }
    Ok(agents)
}

pub fn unique_steps(
    spec: &WorkflowSpec,
    agents: &HashMap<String, WorkflowAgentSpec>,
) -> SubsystemResult<HashMap<String, WorkflowStepSpec>> {
    let mut steps = HashMap::new();
    let mut scheduled_agents = HashSet::new();
    for step in &spec.step {
        validate_runtime_id(&step.id, "step id")?;
        if step.kind != "agent" {
            return Err(SubsystemError::validation(format!(
                "unsupported workflow step kind for {}: {}",
                step.id, step.kind
            )));
        }
        if step.agents.is_empty() {
            return Err(SubsystemError::validation(format!(
                "step.agents must not be empty: {}",
                step.id
            )));
        }
        for agent_uuid in &step.agents {
            if !agents.contains_key(agent_uuid) {
                return Err(SubsystemError::validation(format!(
                    "step {} references unknown workflow agent: {agent_uuid}",
                    step.id
                )));
            }
            if !scheduled_agents.insert(agent_uuid.clone()) {
                return Err(SubsystemError::validation(format!(
                    "agent appears in multiple steps: {agent_uuid}"
                )));
            }
        }
        if steps.insert(step.id.clone(), step.clone()).is_some() {
            return Err(SubsystemError::validation(format!(
                "duplicate step id: {}",
                step.id
            )));
        }
    }
    if steps.is_empty() {
        return Err(SubsystemError::validation(
            "workflow must contain at least one step",
        ));
    }
    for step in steps.values() {
        for need in &step.needs {
            if !steps.contains_key(need) {
                return Err(SubsystemError::validation(format!(
                    "step {} references unknown dependency: {need}",
                    step.id
                )));
            }
        }
    }
    Ok(steps)
}

pub fn validate_step_graph(steps: &HashMap<String, WorkflowStepSpec>) -> SubsystemResult<()> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Visiting,
        Done,
    }

    fn visit(
        step_id: &str,
        steps: &HashMap<String, WorkflowStepSpec>,
        marks: &mut HashMap<String, Mark>,
    ) -> SubsystemResult<()> {
        match marks.get(step_id).copied() {
            Some(Mark::Visiting) => {
                return Err(SubsystemError::validation(format!(
                    "workflow step graph has a cycle at {step_id}"
                )));
            }
            Some(Mark::Done) => return Ok(()),
            None => {}
        }
        marks.insert(step_id.to_string(), Mark::Visiting);
        let step = steps
            .get(step_id)
            .ok_or_else(|| workflow_invariant("validate workflow step graph", "step", step_id))?;
        for need in &step.needs {
            visit(need, steps, marks)?;
        }
        marks.insert(step_id.to_string(), Mark::Done);
        Ok(())
    }

    let mut marks = HashMap::new();
    for step_id in steps.keys() {
        visit(step_id, steps, &mut marks)?;
    }
    Ok(())
}

pub fn validate_context_flow(
    spec: &WorkflowSpec,
    agents: &HashMap<String, WorkflowAgentSpec>,
    steps: &HashMap<String, WorkflowStepSpec>,
) -> SubsystemResult<()> {
    let mut agent_step = HashMap::new();
    for step in steps.values() {
        for agent_uuid in &step.agents {
            agent_step.insert(agent_uuid.clone(), step.id.clone());
        }
    }

    for step in steps.values() {
        let mut allowed = spec
            .workflow
            .planner_context_out
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        for upstream_step_id in transitive_upstream_steps(&step.id, steps)? {
            let upstream_step = steps.get(&upstream_step_id).ok_or_else(|| {
                workflow_invariant(
                    "validate workflow context flow",
                    "upstream step",
                    &upstream_step_id,
                )
            })?;
            for agent_uuid in &upstream_step.agents {
                let agent = agents.get(agent_uuid).ok_or_else(|| {
                    workflow_invariant("validate workflow context flow", "agent", agent_uuid)
                })?;
                allowed.extend(agent.context_out.iter().cloned());
            }
        }
        for agent_uuid in &step.agents {
            let agent = agents.get(agent_uuid).ok_or_else(|| {
                workflow_invariant("validate workflow context flow", "agent", agent_uuid)
            })?;
            for context_uuid in &agent.context_refs {
                if !allowed.contains(context_uuid) {
                    return Err(SubsystemError::validation(format!(
                        "agent {} context_ref {} is not available from planner_context_out or upstream steps",
                        agent.uuid, context_uuid
                    )));
                }
            }
        }
    }

    for agent_uuid in agents.keys() {
        if !agent_step.contains_key(agent_uuid) {
            return Err(SubsystemError::validation(format!(
                "agent is not scheduled by any step: {agent_uuid}"
            )));
        }
    }
    Ok(())
}

fn step_complete(
    step: &WorkflowStepSpec,
    spec: &WorkflowSpec,
    existing_contexts: &HashSet<String>,
) -> bool {
    step.agents.iter().all(|agent_uuid| {
        spec.agent
            .iter()
            .find(|agent| &agent.uuid == agent_uuid)
            .is_some_and(|agent| {
                agent
                    .context_out
                    .iter()
                    .all(|context_uuid| existing_contexts.contains(context_uuid))
            })
    })
}

pub fn require_registered(
    registered_contexts: &HashSet<String>,
    field: &str,
    context_uuid: &str,
) -> SubsystemResult<()> {
    if registered_contexts.contains(context_uuid) {
        Ok(())
    } else {
        Err(SubsystemError::validation(format!(
            "{field} references unregistered context: {context_uuid}"
        )))
    }
}

pub fn validate_runtime_id(value: &str, field: &'static str) -> SubsystemResult<()> {
    if !value.trim().is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && value != "."
        && value != ".."
        && !value.ends_with(".json")
    {
        Ok(())
    } else {
        Err(SubsystemError::validation(format!(
            "invalid {field}: {value}"
        )))
    }
}

fn transitive_upstream_steps(
    step_id: &str,
    steps: &HashMap<String, WorkflowStepSpec>,
) -> SubsystemResult<HashSet<String>> {
    fn collect(
        step_id: &str,
        steps: &HashMap<String, WorkflowStepSpec>,
        out: &mut HashSet<String>,
    ) -> SubsystemResult<()> {
        let step = steps.get(step_id).ok_or_else(|| {
            workflow_invariant("collect upstream workflow steps", "step", step_id)
        })?;
        for need in &step.needs {
            if out.insert(need.clone()) {
                collect(need, steps, out)?;
            }
        }
        Ok(())
    }
    let mut out = HashSet::new();
    collect(step_id, steps, &mut out)?;
    Ok(out)
}

fn workflow_invariant(operation: &'static str, entity: &'static str, id: &str) -> SubsystemError {
    SubsystemError::internal(operation, format!("{entity} {id} is missing"))
}
