use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// $PWD/.prismagent/workflows/{uuid}.json
#[derive(Serialize, Deserialize, Debug)]
pub struct Workflow {
    pub uuid: String,                      // Workflow的唯一标识符
    pub title: String,                     // Workflow的标题或名称，便于识别和管理
    pub content: String,                   // Workflow的内容
    pub metadata: HashMap<String, String>, // Workflow的元数据
    pub created_at: i64,                   // 创建时间戳
    pub updated_at: i64,                   // 更新时间戳
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkflowReadRequest {
    pub uuids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkflowWriteRequest {
    pub workflows: Vec<Workflow>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkflowCreateRequest {
    pub workspace_uuid: String,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkflowReplaceEntry {
    pub uuid: String,
    pub old_data: Vec<u8>,
    pub workflow: Workflow,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkflowReplaceRequest {
    pub entries: Vec<WorkflowReplaceEntry>,
}
