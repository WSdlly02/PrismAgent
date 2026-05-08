use serde::{Deserialize, Serialize};
// $PWD/.prismagent/runs/<run-id>/<agent-name>.json
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Agent {
    pub name: String,                 // Agent的标识符
    pub unit_chain: Vec<String>,      // 存储Agent执行的单元ID列表
    pub unit_head: String,            // 最后一个执行的单元ID
    pub children_agents: Vec<String>, // 召唤的子Agent ID列表
}
