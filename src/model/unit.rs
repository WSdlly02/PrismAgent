use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Serialize, Deserialize, Debug, Clone)]
// $PWD/.prismagent/runs/<run-id>/units/{unit-uuid}.json
pub struct Unit {
    pub uuid: String,
    pub atom_hash: String, // Atom的哈希值，用于索引Atom
    pub kind: UnitKind,
    pub role: UnitRole,
    pub scope: UnitScope,
    pub metadata: HashMap<String, String>,
    pub created_at: u64,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum UnitKind {
    Message,    // 消息单元
    ToolCall,   // 工具调用单元
    ToolResult, // 工具结果单元
    Spawn,      // 召唤子Agent单元
    Result,     // 子Agent结果单元
                // 其他类型可以根据需要添加
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum UnitRole {
    User,
    Assistant,
    System,
    Tool,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub enum UnitScope {
    Workspace,
    Run,
    Agent,
}
