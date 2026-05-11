use crate::model::app::App;
use crate::model::asyncioinstance::AsyncIoHandle;
use crate::model::run::Run;
use std::collections::HashMap;
pub struct Kernel {
    pub app: App,
    pub runtime: Option<KernelRuntime>,
}
pub struct KernelRuntime {
    pub current_run: Run,                        // 当前正在执行的 run
    pub handles: HashMap<String, AsyncIoHandle>, // agent_uuid -> handle
}
