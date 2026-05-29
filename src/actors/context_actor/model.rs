use crate::actors::storage_actor::model::context::Context;
use crate::actors::storage_actor::model::unit::Unit;
use crate::error::SubsystemResult;
use crate::handles::AppHandles;
use genai::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

pub const CONTEXT_ACTOR: &str = "context";

#[derive(Clone)]
pub struct ContextHandle {
    pub tx: mpsc::Sender<ContextMsg>,
}

pub struct ContextActor {
    pub(super) rx: mpsc::Receiver<ContextMsg>,
    pub(super) handles: AppHandles,
}

pub enum ContextMsg {
    ListContexts {
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    ReadContexts {
        uuids: Vec<String>,
        reply: oneshot::Sender<SubsystemResult<Vec<Context>>>,
    },
    WriteContexts {
        contexts: Vec<Context>,
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    Resolve {
        request: ContextResolveRequest,
        reply: oneshot::Sender<SubsystemResult<ContextResolveResponse>>,
    },
    Render {
        request: ContextRenderRequest,
        reply: oneshot::Sender<SubsystemResult<String>>,
    },
    BuildMessages {
        request: BuildMessagesRequest,
        reply: oneshot::Sender<SubsystemResult<Vec<ChatMessage>>>,
    },
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ContextResolveRequest {
    #[serde(default)]
    pub unit_uuids: Vec<String>,
    #[serde(default)]
    pub context_uuids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ContextRenderRequest {
    #[serde(default)]
    pub unit_uuids: Vec<String>,
    #[serde(default)]
    pub context_uuids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct BuildMessagesRequest {
    pub unit_uuids: Vec<String>,
    pub context_uuids: Vec<String>,
    pub user_input: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContextResolveResponse {
    pub units: Vec<Unit>,
    pub contexts: Vec<Context>,
}
