use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Serialize, Deserialize, Debug)]
// $PWD/.prismagent/runs/<run-id>/units/{unit-uuid}.json
pub struct Unit {
    pub uuid: String,
    pub atom_hash: String, // Atom的哈希值，用于索引Atom
    pub kind: UnitKind,
    pub role: UnitRole,
    pub scope: UnitScope,
    pub visibility: UnitVisibility,
    pub metadata: HashMap<String, String>,
    pub created_at: i64,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(rename = "kind")]
pub enum UnitKind {
    #[serde(rename = "user_input")]
    UserInput, // 用户输入单元
    #[serde(rename = "llm_input")]
    LLMInput, // LLM输入单元
    #[serde(rename = "llm_response")]
    LLMResponse, // LLM响应单元
    #[serde(rename = "tool_call")]
    ToolCall, // 工具调用单元
    #[serde(rename = "tool_result")]
    ToolResult, // 工具结果单元
    #[serde(rename = "generic_result")]
    GenericResult, // 通用执行结果单元，供Agent执行任意操作后记录结果使用
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(rename = "role")]
pub enum UnitRole {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "other")]
    Other,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(rename = "scope")]
pub enum UnitScope {
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "agent")]
    Agent,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[serde(rename = "visibility")]
pub enum UnitVisibility {
    #[serde(rename = "internal")]
    Internal, // 仅系统内部可见
    #[serde(rename = "public")]
    Public, // 公开可见
}
