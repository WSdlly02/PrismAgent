use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// $PWD/.prismagent/misc/{name}.json
///
/// MISC 是一个通用的存储结构，可以用于存储各种不适合归类为 Agent、Context、Unit 或 Workflow 的数据。
/// 它具有灵活的内容和元数据字段，允许用户根据需要存储任意类型的信息。
/// 没有uuid字段
#[derive(Serialize, Deserialize, Debug)]
pub struct Misc {
    pub title: String,                     // Misc的标题或名称，便于识别和管理
    pub content: String,                   // Misc的内容
    pub metadata: HashMap<String, String>, // Misc的元数据
    pub created_at: i64,                   // 创建时间戳
    pub updated_at: i64,                   // 更新时间戳
}

#[derive(Debug)]
pub struct MiscReadEntry {
    pub name: String,
    pub misc: Misc,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiscWriteEntry {
    pub name: String,
    pub misc: Misc,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiscWriteRequest {
    pub entries: Vec<MiscWriteEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiscReplaceEntry {
    pub name: String,
    pub old_data: Vec<u8>,
    pub misc: Misc,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MiscReplaceRequest {
    pub entries: Vec<MiscReplaceEntry>,
}
