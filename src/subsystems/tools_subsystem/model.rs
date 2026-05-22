use crate::bus::Bus;
use genai::chat::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::RwLock;

pub type ToolFuture = Pin<Box<dyn Future<Output = String> + Send>>;
pub type AsyncToolExecutor = fn(Bus, PathBuf, Value) -> ToolFuture;

pub struct ToolsSubsystem {
    pub tools: RwLock<Vec<Tool>>,
    // tool name -> tool executor
    pub tools_map: RwLock<HashMap<String, AsyncToolExecutor>>, // may not require Arc if we ensure single-threaded write
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolDispatchRequest {
    pub run_root: PathBuf,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ToolCallDispatchRequest {
    pub run_root: PathBuf,
    pub tool_call: genai::chat::ToolCall,
}
