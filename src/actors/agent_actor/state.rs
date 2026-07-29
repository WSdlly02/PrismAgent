use crate::actors::agent_actor::model::{AgentStatus, SendMessageRequest};
use crate::actors::storage_actor::model::agent::Agent;
use genai::chat::ToolCall;
use std::sync::Arc;

/// Persisted Agent data and its process-local execution state.
///
/// Keeping them in one map entry prevents the actor from having to maintain
/// parallel agent/workspace/state maps.
pub(crate) struct AgentEntry {
    pub workspace_uuid: String,
    pub agent: Agent,
    pub state: AgentState,
}

impl AgentEntry {
    pub fn idle(workspace_uuid: String, agent: Agent) -> Self {
        Self {
            workspace_uuid,
            agent,
            state: AgentState::Idle,
        }
    }
}

/// State carried across consecutive attempts to produce one valid LLM output.
///
/// A malformed tool-call output starts a repair continuation and carries this
/// context into the next LLM invocation. The context resets after the first
/// structurally valid output, before normal tool or auto-loop continuation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TurnContext {
    pub malformed_tool_call_retries: u8,
}

/// Runtime state is represented as a tagged union so invalid combinations such
/// as `Idle + active_tool_batch` cannot be constructed.
#[derive(Clone, Debug)]
pub(crate) enum AgentState {
    Idle,
    RunningLlm {
        inference_uuid: String,
        turn: TurnContext,
    },
    WaitingApproval {
        request_uuid: String,
        tool_calls: Arc<[ToolCall]>,
        auto_approved_mask: u64,
        manual_approval_mask: u64,
        turn: TurnContext,
    },
    RunningTool {
        job_uuid: String,
        tool_calls: Arc<[ToolCall]>,
        turn: TurnContext,
    },
}

impl AgentState {
    pub fn status(&self) -> AgentStatus {
        match self {
            Self::Idle => AgentStatus::Idle,
            Self::RunningLlm { .. } => AgentStatus::RunningLlm,
            Self::WaitingApproval { .. } => AgentStatus::WaitingApproval,
            Self::RunningTool { .. } => AgentStatus::RunningTool,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

/// A handler describes the next step; it does not mutate runtime state or
/// launch the next background task itself.
pub(crate) enum NextAction {
    Finish,
    /// Start a new inference turn, when user sends a message to the agent.
    StartInference {
        request: SendMessageRequest,
        turn: TurnContext,
    },
    /// Continue an inference turn, when the tool has returned a response.
    ContinueInference {
        turn: TurnContext,
    },
    RequestApproval {
        tool_calls: Arc<[ToolCall]>,
        auto_approved_mask: u64,
        manual_approval_mask: u64,
        turn: TurnContext,
    },
    StartTools {
        tool_calls: Arc<[ToolCall]>,
        approval_mask: ApprovalMask,
        denied_reason: String,
        turn: TurnContext,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApprovalMask(u64);

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool_call() -> ToolCall {
        ToolCall {
            call_id: "call".to_string(),
            fn_name: "tool".to_string(),
            fn_arguments: json!({}),
            thought_signatures: None,
        }
    }

    #[test]
    fn state_status_is_derived_from_the_variant() {
        assert_eq!(AgentState::Idle.status(), AgentStatus::Idle);
        assert!(AgentState::Idle.is_idle());

        let running_llm = AgentState::RunningLlm {
            inference_uuid: "inference".to_string(),
            turn: TurnContext::default(),
        };
        let waiting_approval = AgentState::WaitingApproval {
            request_uuid: "approval".to_string(),
            tool_calls: vec![tool_call()].into(),
            auto_approved_mask: 0,
            manual_approval_mask: 1,
            turn: TurnContext::default(),
        };
        let running_tool = AgentState::RunningTool {
            job_uuid: "job".to_string(),
            tool_calls: vec![tool_call()].into(),
            turn: TurnContext::default(),
        };

        assert_eq!(running_llm.status(), AgentStatus::RunningLlm);
        assert_eq!(waiting_approval.status(), AgentStatus::WaitingApproval);
        assert_eq!(running_tool.status(), AgentStatus::RunningTool);
        assert!(!running_llm.is_idle());
        assert!(!waiting_approval.is_idle());
        assert!(!running_tool.is_idle());
    }
}
