use crate::model::app::App;
use crate::model::asyncioinstance::AsyncIoHandle;
use crate::model::run::Run;
use crate::model::unit::Unit;
use std::collections::HashMap;

pub struct Kernel {
    pub app: App,
    pub runtime: Option<KernelRuntime>,
}

pub struct KernelRuntime {
    pub current_run: Run,                        // 当前正在执行的 run
    pub handles: HashMap<String, AsyncIoHandle>, // asyncioinstance_uuid -> handle
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
