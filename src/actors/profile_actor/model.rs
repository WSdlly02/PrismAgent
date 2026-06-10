use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

use crate::error::SubsystemResult;

pub const PROFILE_ACTOR: &str = "profile";

#[derive(Clone)]
pub struct ProfileHandle {
    pub tx: mpsc::Sender<ProfileMsg>,
}

pub struct ProfileActor {
    pub(super) rx: mpsc::Receiver<ProfileMsg>,
    pub(super) root: PathBuf,
    pub(super) profiles: HashMap<String, Profile>, // profile_name -> Profile
}

pub enum ProfileMsg {
    ListProfiles {
        reply: oneshot::Sender<SubsystemResult<Vec<String>>>,
    },
    GetProfile {
        name: String,
        reply: oneshot::Sender<SubsystemResult<Profile>>,
    },
    GetModelConfig {
        profile_name: String,
        reply: oneshot::Sender<SubsystemResult<FinalModelConfig>>,
    },
    GetPrompts {
        profile_name: String,
        reply: oneshot::Sender<SubsystemResult<PromptsConfigSection>>,
    },
    GetTools {
        profile_name: String,
        reply: oneshot::Sender<SubsystemResult<ToolsConfigSection>>,
    },
}

/// ~/.prismagent/profiles/{profile_name}.toml
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Profile {
    pub name: String, // e.g. "default"，"planner", "executor"
    pub model: ModelConfigSection,
    pub prompts: PromptsConfigSection,
    pub tools: ToolsConfigSection,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ModelConfigSection {
    pub provider: String,    // "deepseek"
    pub model_name: String,  // "deepseek-v4-flash"
    pub api_key_env: String, // name of env var containing API key, e.g. "DEEPSEEK_API_KEY"
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FinalModelConfig {
    pub provider: String,
    pub model_name: String,
    pub api_key: String,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PromptsConfigSection {
    pub system: SystemPromptConfig,
    pub auto_loop: bool, // whether to automatically loop until the "finish" tool is called, without asking for user confirmation after each tool call
    pub auto_loop_message: String, // if auto_loop is true, this message will be sent to the user after each tool call, e.g. "The agent has called a tool. Do you want to let it continue to the next step? If you want to stop it, please reply with 'stop'."
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SystemPromptConfig {
    pub identity: String,       // e.g. "You are a helpful assistant."
    pub behavior: String, // e.g. "If the task requires using any of the above skills or tools, please use them. If not, just answer the question directly. If the task is complete, call the "finish" tool with the final answer."
    pub response_style: String, // 需要存在吗？
    pub capabilities: String, // e.g. "{skills} {tools}"，在实际使用时会被替换成具体的技能和工具信息
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolsConfigSection {
    pub yolo: bool, // whether to use YOLO for tool calls, will short-circuit the auto_approve list and directly approve all tool calls
    pub available_tools: Vec<String>, // list of tool names that the agent can use, e.g. ["search", "calculator"]
    pub auto_approve: Vec<String>, // list of tool names that can be auto-approved without user confirmation
}
