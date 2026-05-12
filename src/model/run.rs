use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub struct Run {
    pub root: PathBuf,             // $PWD/.prismagent/runs/<run-id>
    pub run_metadata: RunMetadata, // 从 $PWD/.prismagent/runs/<run-id>/metadata.json 读取
    pub run_lock: Option<RunLock>, // 运行锁，表示当前 run 是否正在被执行
}
/// $PWD/.prismagent/runs/{run-uuid}/metadata.json
#[derive(Serialize, Deserialize, Debug)]
pub struct RunMetadata {
    pub uuid: String,
    pub title: String,
    pub status: RunStatus,
    pub root_agent: String,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub enum RunStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "archived")]
    Archived,
}
// $PWD/.prismagent/runs/<run-id>/run.lock
#[derive(Serialize, Deserialize, Debug)]
pub struct RunLock {
    pub pid: u32,             // 锁定进程的 PID
    pub owner: String,        // 锁定者的标识
    pub locked_at: i64,       // 锁定时间戳
    pub hostname: String,     // 锁定者的主机名
    pub note: Option<String>, // 锁定备注信息
}
pub struct RunSummary {
    pub uuid: String,
    pub title: String,
    pub locked: bool,
    pub status: RunStatus,
    pub updated_at: i64,
}
