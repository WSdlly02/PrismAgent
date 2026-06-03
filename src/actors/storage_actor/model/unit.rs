use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// $PWD/.prismagent/units/{uuid}.json
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Unit {
    pub uuid: String,
    pub visibility: UnitVisibility,

    pub content: genai::chat::ChatMessage, // 直接使用 genai 的 ChatMessage 结构
    pub token_usage: Option<genai::chat::Usage>, // LLM 返回的真实请求级 token usage, 只有assistant消息才会有值

    pub metadata: HashMap<String, String>,
    pub created_at: i64,
}

impl Unit {
    pub fn from_user_text(content: String) -> Self {
        let message = genai::chat::ChatMessage::user(content);
        let unit = Self::from_chat_message(message);
        unit
    }

    pub fn from_chat_message(message: genai::chat::ChatMessage) -> Self {
        Self::from_chat_message_with_usage(message, None)
    }

    pub fn from_chat_message_with_usage(
        message: genai::chat::ChatMessage,
        token_usage: Option<genai::chat::Usage>,
    ) -> Self {
        Self {
            uuid: Uuid::now_v7().to_string(),
            visibility: UnitVisibility::Public,
            content: message,
            token_usage,
            metadata: HashMap::new(),
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn to_chat_message(&self) -> genai::chat::ChatMessage {
        self.content.clone()
    }
}

impl From<String> for Unit {
    fn from(content: String) -> Self {
        Self::from_user_text(content)
    }
}

impl From<genai::chat::ChatMessage> for Unit {
    fn from(message: genai::chat::ChatMessage) -> Self {
        Self::from_chat_message(message)
    }
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
