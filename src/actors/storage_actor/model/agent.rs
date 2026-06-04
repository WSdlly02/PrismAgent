use crate::actors::storage_actor::model::unit::Unit;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// $PWD/.prismagent/agents/{uuid}.json
#[derive(Serialize, Deserialize, Debug)]
pub struct Agent {
    pub uuid: String,              // Agent的唯一标识符
    pub name: String,              // Agent的展示名称，最好不要重复
    pub profile: String,           // Agent的配置文件名称，例如 "default"、"custom-profile-1" 等
    pub unit_chain: Vec<String>,   // 存储Agent执行的单元ID列表
    pub unit_head: String,         // 最后一个执行的单元ID
    pub context_refs: Vec<String>, // 上下文引用列表，存储与Agent相关的上下文ID或名称
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

    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentReadRequest {
    pub uuids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentWriteRequest {
    pub agents: Vec<Agent>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AgentAppendUnitsRequest {
    pub agent_uuid: String,
    pub units: Vec<Unit>,
}

/// $PWD/.prismagent/agents/{uuid}.lock
#[derive(Serialize, Deserialize, Debug)]
pub struct AgentLock {
    pub pid: u32,             // 锁定进程的 PID
    pub owner: String,        // 锁定者的标识
    pub locked_at: i64,       // 锁定时间戳
    pub hostname: String,     // 锁定者的主机名
    pub note: Option<String>, // 锁定备注信息
}
