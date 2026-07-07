use crate::actors::profile_actor::model::FinalModelConfig;
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::SubsystemResult;
use genai::chat::Tool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

pub const LLM_ACTOR: &str = "llm";

#[derive(Clone)]
pub struct LlmHandle {
    pub tx: mpsc::Sender<LlmMsg>,
}

pub struct LlmActor {
    pub(super) rx: mpsc::Receiver<LlmMsg>,
    pub(super) clients: HashMap<String, genai::Client>,
    pub(super) inflight: HashMap<String, tokio::task::JoinHandle<()>>,
}

pub enum LlmMsg {
    Infer {
        request: LlmInferRequest,
        reply: oneshot::Sender<SubsystemResult<LlmInferResponse>>,
    },
    Cancel {
        inference_uuid: String,
        reply: oneshot::Sender<SubsystemResult<bool>>,
    },
}

pub struct LlmInferRequest {
    pub inference_uuid: String,
    pub model: FinalModelConfig,
    pub units: Vec<Unit>,
    pub tools: Vec<Tool>,
    pub stream_tx: mpsc::Sender<LlmStreamEvent>,
}

pub struct LlmInferResponse {
    pub output_unit: Unit,
    pub is_tool_calls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmStreamEvent {
    Started,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallDelta { name: Option<String> },
    Finished,
}
