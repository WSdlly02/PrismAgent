use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// $PWD/.prismagent/units/{uuid}.json
#[derive(Serialize, Deserialize, Debug)]
pub struct Unit {
    pub uuid: String,
    pub visibility: UnitVisibility,

    pub content: genai::chat::ChatMessage, // 直接使用 genai 的 ChatMessage 结构
    pub estimated_tokens: u32,             // 估算 token 数量

    pub metadata: HashMap<String, String>,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UnitReadRequest {
    pub uuids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UnitWriteRequest {
    pub units: Vec<Unit>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
#[serde(rename = "visibility")]
pub enum UnitVisibility {
    #[serde(rename = "internal")]
    Internal, // 对用户不可见
    #[serde(rename = "public")]
    Public, // 对用户可见
}
