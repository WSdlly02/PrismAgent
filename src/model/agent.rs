use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// $PWD/.prismagent/runs/{run-uuid}/agents/{uuid}.json
#[derive(Serialize, Deserialize, Debug)]
pub struct Agent {
    pub uuid: String,                 // Agent的唯一标识符
    pub name: String,                 // Agent的展示名称，最好不要重复
    pub unit_chain: Vec<String>,      // 存储Agent执行的单元ID列表
    pub unit_head: String,            // 最后一个执行的单元ID
    pub children_agents: Vec<String>, // 召唤的子Agent UUID列表
    /// 快照UID到单元UUID列表的映射
    ///
    /// 快照UID可以是一个时间戳字符串，也可以是一个用户指定的名称，例如
    ///
    /// snapshot-before-tool-call、snapshot-after-llm-response 等
    ///
    /// 如 "snapshot-20240601T120000Z" -> ["unit-uuid-1", "unit-uuid-2", ...]
    ///
    /// 但必须保证快照UID的唯一性，不能重复使用相同的快照UID来表示不同的单元链状态。
    pub snapshots: HashMap<String, Vec<String>>,
}
