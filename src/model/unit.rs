use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// $PWD/.prismagent/runs/{run-uuid}/units/{unit-uuid}.json
///
/// Unit is a descriptor of an immutable Atom.
/// It does not own content. It only describes how the Atom should be interpreted
/// inside an agent's unit_chain.
#[derive(Serialize, Deserialize, Debug)]
pub struct Unit {
    pub uuid: String,
    pub atom_hash: String, // Atom的哈希值，用于索引Atom
    pub role: UnitRole,
    pub scope: UnitScope,
    pub visibility: UnitVisibility,
    pub metadata: HashMap<String, String>,
    pub created_at: i64,
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
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
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
#[serde(rename = "scope")]
pub enum UnitScope {
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "agent")]
    Agent,
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
#[serde(rename = "visibility")]
pub enum UnitVisibility {
    #[serde(rename = "internal")]
    Internal, // 仅系统内部可见
    #[serde(rename = "public")]
    Public, // 公开可见
}
