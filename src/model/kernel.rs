use crate::model::app::App;
use crate::model::asyncioinstance::{
    AsyncIoHandle, AsyncIoInstanceExecutionMode, AsyncIoInstanceRole,
};
use crate::model::run::Run;
use crate::model::unit::Unit;
use std::collections::HashMap;

pub struct Kernel {
    pub app: App,                       // 不可变配置文件状态，初始化时读取
    pub llm_client: genai::Client,      // 不可变 LLM 客户端，初始化时创建
    pub runtime: Option<KernelRuntime>, // 可变状态，执行过程中更新
}

pub struct KernelRuntime {
    pub current_run: Run,                             // 当前正在执行的 run
    pub handles: HashMap<String, AsyncIoHandleEntry>, // asyncioinstance_uuid -> handle entry
}

pub struct AsyncIoHandleEntry {
    pub run_uuid: String,
    pub owner: AsyncIoOwner,
    pub role: AsyncIoInstanceRole,
    pub execution_mode: AsyncIoInstanceExecutionMode,
    pub handle: AsyncIoHandle,
}

pub enum AsyncIoOwner {
    Kernel,
    Agent { agent_uuid: String },
}

/// AsyncIoInstance -> Kernel 的内部事件。
///
/// 这是内核私有事件流，不直接暴露给 TUI。实例完成一次处理后把 Vec<Unit>
/// 交还给 kernel，由 kernel 串行执行 output pipeline、更新 runtime 并转发给 shell。
pub struct InstanceToKernelEvent {
    pub correlation_uuid: Option<String>,
    pub run_uuid: String,
    pub agent_uuid: String,
    pub asyncioinstance_uuid: String,
    pub stream: InstanceStream,
}

pub enum InstanceStream {
    Output(Vec<Unit>),
    Error(Vec<Unit>),
}
