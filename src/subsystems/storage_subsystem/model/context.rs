use serde::{Deserialize, Serialize};

/// $PWD/.prismagent/contexts/{uuid}.json
#[derive(Serialize, Deserialize, Debug)]
pub struct Context {
    pub uuid: String,    // Context的唯一标识符
    pub title: String,   // Context的标题或名称，便于识别和管理
    pub content: String, // Context的内容，遵循Model Context Exchange Standard规范
    pub created_at: i64, // 创建时间戳
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContextReadRequest {
    pub uuid: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ContextWriteRequest {
    pub context: Context,
}
