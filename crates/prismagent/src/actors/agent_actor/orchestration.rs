use crate::actors::agent_actor::model::{
    AgentActor, AgentFailureStage, AgentInferenceOutput, AgentTaskError, AgentTaskResult,
    ApproveRequest, PendingApproval, SendMessageRequest, ToolBatchOutput,
};
use crate::actors::agent_actor::pipeline::{
    RunToolBatchRequest, auto_approval_mask, clone_tool_calls, run_llm_continuation,
    run_llm_inference, run_tool_batch, tool_batch_is_auto_approved, tool_calls_sound,
    tool_response_units,
};
use crate::actors::agent_actor::runtime::effective_approval_mask;
use crate::actors::agent_actor::state::{AgentState, ApprovalMask, NextAction, TurnContext};
use crate::actors::shell_actor::model::WsEvent;
use crate::actors::storage_actor::model::unit::{Unit, UnitVisibility};
use crate::error::{ErrorClass, ExternalKind, SubsystemError, SubsystemResult};
use crate::id::AgentAddr;
use genai::chat::ToolCall;
use std::sync::Arc;
use uuid::Uuid;

const MAX_MALFORMED_TOOL_CALL_RETRIES: u8 = 2;

// ┌──────────────────────────────────────────────────────────────────────────┐
// │                     AgentActor State Machine                             │
// │                                                                          │
// │  Idle ── send_message / initial task ──> RunningLlm                      │
// │                                                                          │
// │  RunningLlm ── malformed tool calls (repair) ──────────> RunningLlm      │
// │             ├── text + auto_loop ──────────────────────> RunningLlm      │
// │             ├── text + no auto_loop ───────────────────> Idle            │
// │             ├── tool calls (auto-approved) ────────────> RunningTool     │
// │             └── tool calls (approval required) ────────> WaitingApproval │
// │                                                                          │
// │  WaitingApproval ── approve / deny ────────────────────> RunningTool     │
// │  RunningTool ── continue_loop ─────────────────────────> RunningLlm      │
// │              └── no continuation ──────────────────────> Idle            │
// │                                                                          │
// │  Cancel:                                                                 │
// │    RunningLlm → cancel LLM → Idle                                        │
// │    WaitingApproval / RunningTool → run all-denied tool batch → Idle      │
// └──────────────────────────────────────────────────────────────────────────┘

impl AgentActor {
    pub(super) fn apply_next_action(
        &mut self,
        addr: &AgentAddr,
        action: NextAction,
    ) -> SubsystemResult<()> {
        match action {
            NextAction::Finish => self.transition_to(addr, AgentState::Idle),
            NextAction::StartInference { request, turn } => {
                self.start_inference(addr, request, turn)
            }
            NextAction::ContinueInference { turn } => self.continue_inference(addr, turn),
            NextAction::RequestApproval {
                tool_calls,
                auto_approved_mask,
                manual_approval_mask,
                turn,
            } => {
                let request_uuid = Uuid::now_v7().to_string();
                let tool_count = tool_calls.len();
                self.transition_to(
                    addr,
                    AgentState::WaitingApproval {
                        request_uuid: request_uuid.clone(),
                        tool_calls,
                        auto_approved_mask,
                        manual_approval_mask,
                        turn,
                    },
                )?;
                self.emit_agent_event(
                    addr,
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
            NextAction::StartTools {
                tool_calls,
                approval_mask,
                denied_reason,
                turn,
            } => {
                let profile_name = self.agent(addr)?.profile.clone();
                let job_uuid = Uuid::now_v7().to_string();
                self.transition_to(
                    addr,
                    AgentState::RunningTool {
                        job_uuid: job_uuid.clone(),
                        tool_calls: tool_calls.clone(),
                        turn,
                    },
                )?;

                let handles = self.handles.clone();
                let task_addr = addr.clone();
                tokio::spawn(async move {
                    let result = run_tool_batch(
                        &handles,
                        RunToolBatchRequest {
                            addr: task_addr.clone(),
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
                        .tool_batch_complete(task_addr, job_uuid, result)
                        .await;
                });
                Ok(())
            }
        }
    }

    fn start_inference(
        &mut self,
        addr: &AgentAddr,
        request: SendMessageRequest,
        turn: TurnContext,
    ) -> SubsystemResult<()> {
        let agent = self.agent(addr)?;
        let unit_uuids = agent.unit_chain.clone();
        let profile_name = agent.profile.clone();
        let inference_uuid = Uuid::now_v7().to_string();
        self.transition_to(
            addr,
            AgentState::RunningLlm {
                inference_uuid: inference_uuid.clone(),
                turn,
            },
        )?;

        let handles = self.handles.clone();
        let task_addr = addr.clone();
        let task_inference_uuid = inference_uuid.clone();
        tokio::spawn(async move {
            let result =
                run_llm_inference(&handles, unit_uuids, request, profile_name, inference_uuid)
                    .await;
            let _ = handles
                .agent
                .inference_complete(task_addr, task_inference_uuid, result)
                .await;
        });
        Ok(())
    }

    fn continue_inference(&mut self, addr: &AgentAddr, turn: TurnContext) -> SubsystemResult<()> {
        let agent = self.agent(addr)?;
        let unit_uuids = agent.unit_chain.clone();
        let profile_name = agent.profile.clone();
        let inference_uuid = Uuid::now_v7().to_string();
        self.transition_to(
            addr,
            AgentState::RunningLlm {
                inference_uuid: inference_uuid.clone(),
                turn,
            },
        )?;

        let handles = self.handles.clone();
        let task_addr = addr.clone();
        let task_inference_uuid = inference_uuid.clone();
        tokio::spawn(async move {
            let result = run_llm_continuation(
                &handles,
                task_addr.clone(),
                unit_uuids,
                profile_name,
                inference_uuid,
            )
            .await;
            let _ = handles
                .agent
                .inference_complete(task_addr, task_inference_uuid, result)
                .await;
        });
        Ok(())
    }

    pub(super) async fn finish_inference(
        &mut self,
        addr: &AgentAddr,
        inference_uuid: &str,
        result: AgentTaskResult<AgentInferenceOutput>,
    ) {
        let turn = match self.state(addr) {
            Ok(AgentState::RunningLlm {
                inference_uuid: active_uuid,
                turn,
            }) if active_uuid == inference_uuid => *turn,
            _ => return,
        };

        let decision = self.decide_inference_completion(addr, turn, result).await;
        self.complete_background_action(addr, inference_uuid, decision);
    }

    async fn decide_inference_completion(
        &mut self,
        addr: &AgentAddr,
        turn: TurnContext,
        result: AgentTaskResult<AgentInferenceOutput>,
    ) -> AgentTaskResult<NextAction> {
        let output = result?;
        let is_tool_calls = output.is_tool_calls;
        let tool_calls: Arc<[ToolCall]> = if is_tool_calls {
            output
                .units
                .last()
                .map(clone_tool_calls)
                .unwrap_or_default()
        } else {
            Arc::new([])
        };

        if is_tool_calls && !tool_calls_sound(&tool_calls) {
            return self
                .repair_malformed_tool_calls(addr, turn)
                .await
                .map_err(|source| AgentTaskError::new(AgentFailureStage::RepairToolCalls, source));
        }

        self.commit_units(addr, output.units)
            .await
            .map_err(|source| AgentTaskError::new(AgentFailureStage::CommitLlmOutput, source))?;

        // A structurally valid output ends any malformed-call repair chain.
        // Normal tool and auto-loop continuations start with a fresh budget.
        let turn = TurnContext::default();
        if is_tool_calls {
            return self
                .decide_tool_call_approval(addr, tool_calls, turn)
                .await
                .map_err(|source| {
                    AgentTaskError::new(AgentFailureStage::PrepareToolBatch, source)
                });
        }

        if self.agent(addr).is_ok_and(|agent| agent.auto_loop) {
            return self
                .prepare_auto_loop(addr, turn)
                .await
                .map_err(|source| AgentTaskError::new(AgentFailureStage::PrepareAutoLoop, source));
        }

        Ok(NextAction::Finish)
    }

    async fn repair_malformed_tool_calls(
        &mut self,
        addr: &AgentAddr,
        mut turn: TurnContext,
    ) -> SubsystemResult<NextAction> {
        let retry = if turn.malformed_tool_call_retries < MAX_MALFORMED_TOOL_CALL_RETRIES {
            turn.malformed_tool_call_retries += 1;
            Some(turn.malformed_tool_call_retries)
        } else {
            None
        };

        let text = match retry {
            Some(attempt) => format!(
                "[PrismAgent] Your previous tool call was malformed. Tool arguments must be a JSON object, and call_id/fn_name must be non-empty. Retry the tool call with valid arguments. Repair attempt {attempt}/{MAX_MALFORMED_TOOL_CALL_RETRIES}."
            ),
            None => format!(
                "[PrismAgent] Tool-call repair stopped after {MAX_MALFORMED_TOOL_CALL_RETRIES} malformed attempts. Please inspect the previous output and continue manually."
            ),
        };
        self.commit_units(addr, vec![Unit::from_user_text(text)])
            .await?;

        if retry.is_some() {
            Ok(NextAction::ContinueInference { turn })
        } else {
            Err(SubsystemError::external(
                ExternalKind::Llm,
                ErrorClass::Internal,
                "LLM produced malformed tool calls repeatedly",
                false,
            ))
        }
    }

    pub(super) async fn finish_tool_batch(
        &mut self,
        addr: &AgentAddr,
        job_uuid: &str,
        result: AgentTaskResult<ToolBatchOutput>,
    ) {
        let (active_tool_calls, turn) = match self.state(addr) {
            Ok(AgentState::RunningTool {
                job_uuid: active_uuid,
                tool_calls,
                turn,
            }) if active_uuid == job_uuid => (tool_calls.clone(), *turn),
            _ => return,
        };

        let decision = match result {
            Ok(output) => self.decide_tool_batch_success(addr, turn, output).await,
            Err(error) => {
                let units = tool_response_units(
                    &active_tool_calls,
                    "error",
                    &format!("tool batch failed: {}", error.source),
                );
                if let Err(source) = self.commit_units(addr, units).await {
                    self.emit_task_failure(
                        addr,
                        job_uuid,
                        AgentTaskError::new(AgentFailureStage::CommitToolOutput, source),
                    );
                }
                Err(error)
            }
        };
        self.complete_background_action(addr, job_uuid, decision);
    }

    async fn decide_tool_batch_success(
        &mut self,
        addr: &AgentAddr,
        turn: TurnContext,
        output: ToolBatchOutput,
    ) -> AgentTaskResult<NextAction> {
        self.commit_units(addr, output.units)
            .await
            .map_err(|source| AgentTaskError::new(AgentFailureStage::CommitToolOutput, source))?;

        if output.continue_loop {
            Ok(NextAction::ContinueInference { turn })
        } else {
            Ok(NextAction::Finish)
        }
    }

    fn complete_background_action(
        &mut self,
        addr: &AgentAddr,
        correlation_id: &str,
        decision: AgentTaskResult<NextAction>,
    ) {
        let action = match decision {
            Ok(action) => action,
            Err(error) => {
                self.emit_task_failure(addr, correlation_id, error);
                NextAction::Finish
            }
        };

        if let Err(source) = self.apply_next_action(addr, action) {
            self.emit_task_failure(
                addr,
                correlation_id,
                AgentTaskError::new(AgentFailureStage::ApplyNextAction, source),
            );
            let _ = self.apply_next_action(addr, NextAction::Finish);
        }
    }

    async fn prepare_auto_loop(
        &mut self,
        addr: &AgentAddr,
        turn: TurnContext,
    ) -> SubsystemResult<NextAction> {
        let auto_loop_message = self.agent(addr)?.auto_loop_message.clone();
        let mut unit = Unit::from_chat_message(genai::chat::ChatMessage::user(auto_loop_message));
        unit.visibility = UnitVisibility::Internal;
        self.commit_units(addr, vec![unit]).await?;
        Ok(NextAction::ContinueInference { turn })
    }

    pub(super) async fn approve_request(&mut self, request: ApproveRequest) -> SubsystemResult<()> {
        self.ensure_loaded(&request.addr).await?;
        let action = match self.state(&request.addr)?.clone() {
            AgentState::WaitingApproval {
                request_uuid,
                tool_calls,
                auto_approved_mask,
                manual_approval_mask,
                turn,
            } => {
                if request_uuid != request.request_uuid {
                    return Err(SubsystemError::validation(format!(
                        "unknown approval request: {}",
                        request.request_uuid
                    )));
                }
                NextAction::StartTools {
                    tool_calls,
                    approval_mask: effective_approval_mask(
                        auto_approved_mask,
                        manual_approval_mask,
                        request.approval_mask,
                    ),
                    denied_reason: "user denied tool execution".to_string(),
                    turn,
                }
            }
            _ => {
                return Err(SubsystemError::validation(format!(
                    "no pending approval request: {}",
                    request.request_uuid
                )));
            }
        };
        self.apply_next_action(&request.addr, action)
    }

    pub(super) async fn cancel(&mut self, addr: &AgentAddr) -> SubsystemResult<()> {
        self.ensure_loaded(addr).await?;
        let action = match self.state(addr)?.clone() {
            AgentState::RunningLlm { inference_uuid, .. } => {
                let _ = self.handles.llm.cancel(inference_uuid).await?;
                NextAction::Finish
            }
            AgentState::WaitingApproval {
                tool_calls, turn, ..
            } => NextAction::StartTools {
                tool_calls,
                approval_mask: ApprovalMask::none(),
                denied_reason: "tool execution cancelled by user".to_string(),
                turn,
            },
            AgentState::RunningTool {
                job_uuid,
                tool_calls,
                turn,
            } => {
                let _ = self.handles.tools.cancel(job_uuid).await?;
                NextAction::StartTools {
                    tool_calls,
                    approval_mask: ApprovalMask::none(),
                    denied_reason: "tool execution cancelled by user".to_string(),
                    turn,
                }
            }
            AgentState::Idle => return Ok(()),
        };
        self.apply_next_action(addr, action)
    }

    async fn decide_tool_call_approval(
        &mut self,
        addr: &AgentAddr,
        tool_calls: Arc<[ToolCall]>,
        turn: TurnContext,
    ) -> SubsystemResult<NextAction> {
        if tool_calls.is_empty() {
            return Ok(NextAction::Finish);
        }
        if tool_calls.len() > 64 {
            return Err(SubsystemError::validation(format!(
                "tool batch cannot contain more than 64 calls: {}",
                tool_calls.len()
            )));
        }
        let profile_name = self.agent(addr)?.profile.clone();
        let tools_config = self.handles.profile.tools(profile_name).await?;
        let all_mask = ApprovalMask::all_for(tool_calls.len()).bits();
        let auto_approved_mask = auto_approval_mask(&tools_config, &tool_calls) & all_mask;
        if tool_batch_is_auto_approved(&tools_config, &tool_calls) {
            Ok(NextAction::StartTools {
                tool_calls,
                approval_mask: ApprovalMask::from_bits(all_mask),
                denied_reason: "tool execution was auto-approved".to_string(),
                turn,
            })
        } else {
            let manual_approval_mask = all_mask & !auto_approved_mask;
            Ok(NextAction::RequestApproval {
                tool_calls,
                auto_approved_mask,
                manual_approval_mask,
                turn,
            })
        }
    }

    fn transition_to(&mut self, addr: &AgentAddr, state: AgentState) -> SubsystemResult<()> {
        let status = state.status();
        self.entry_mut(addr)?.state = state;
        self.emit_agent_event(
            addr,
            WsEvent::StatusChanged {
                status: status.clone(),
            },
        );
        self.emit_workspace_event(
            &addr.workspace_uuid,
            WsEvent::AgentStatusChanged {
                agent_uuid: addr.agent_uuid.clone(),
                status,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::agent_actor::model::AgentStatus;
    use crate::actors::agent_actor::state::AgentEntry;
    use crate::actors::storage_actor::model::agent::Agent;
    use serde_json::json;
    use std::collections::HashMap;
    use tokio::sync::mpsc;

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

    fn test_actor() -> AgentActor {
        let (_tx, rx) = mpsc::channel(1);
        let mut actor = AgentActor::load(rx, crate::handles::test_handles());
        actor.entries.insert(
            AgentAddr::new("workspace", "agent"),
            AgentEntry::idle(test_agent("agent")),
        );
        actor
    }

    #[test]
    fn router_is_the_single_state_transition_boundary() {
        let mut actor = test_actor();
        let tool_call = ToolCall {
            call_id: "call".to_string(),
            fn_name: "tool".to_string(),
            fn_arguments: json!({}),
            thought_signatures: None,
        };

        actor
            .apply_next_action(
                &AgentAddr::new("workspace", "agent"),
                NextAction::RequestApproval {
                    tool_calls: vec![tool_call].into(),
                    auto_approved_mask: 0,
                    manual_approval_mask: 1,
                    turn: TurnContext::default(),
                },
            )
            .unwrap();

        let state = actor.state(&AgentAddr::new("workspace", "agent")).unwrap();
        assert_eq!(state.status(), AgentStatus::WaitingApproval);
        assert!(matches!(
            state,
            AgentState::WaitingApproval {
                tool_calls,
                auto_approved_mask: 0,
                manual_approval_mask: 1,
                ..
            } if tool_calls.len() == 1
        ));

        actor
            .apply_next_action(&AgentAddr::new("workspace", "agent"), NextAction::Finish)
            .unwrap();
        assert!(
            actor
                .state(&AgentAddr::new("workspace", "agent"))
                .unwrap()
                .is_idle()
        );
    }
}
