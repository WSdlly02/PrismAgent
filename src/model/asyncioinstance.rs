use crate::model::event::InstanceToKernelEvent;
use crate::model::unit::Unit;
use anyhow::Result;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

/// 异步IO实例的元结构体，包含实例和控制句柄
pub struct AsyncIoBox {
    instance: AsyncIoInstance,
    handle: AsyncIoHandle,
}
/// 异步IO的实例
pub struct AsyncIoInstance {
    pub uuid: String,
    pub role: AsyncIoInstanceRole, // 实例的角色，决定了它的行为和权限
    pub execution_mode: AsyncIoInstanceExecutionMode, // 实例的执行模式，决定了它的生命周期和调度方式

    pub stdin: mpsc::Receiver<Vec<Unit>>, // 数据输入通道
    pub signal_rx: mpsc::Receiver<InstanceSignal>, // Kernel -> Instance 的控制信号
    pub kernel_tx: mpsc::Sender<InstanceToKernelEvent>, // Instance -> Kernel 的事件通道

    pub metadata: HashMap<String, String>, // 实例的元数据, 可以存储任意键值对，供内核和Agent使用
}
/// 异步IO的控制句柄，提供给内核使用
pub struct AsyncIoHandle {
    pub stdin: mpsc::Sender<Vec<Unit>>,          // 数据输入通道
    pub signal_tx: mpsc::Sender<InstanceSignal>, // Kernel -> Instance 的控制信号
}
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum AsyncIoInstanceRole {
    Unknown,
    LLM,  // 收到Vec<Unit>后会被解析成LLM输入，输出会被解析成LLM输出
    Tool, // 收到Vec<Unit>后会判断尾部Unit是否批准、解析工具输入，输出会被解析成工具输出
}
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum AsyncIoInstanceExecutionMode {
    Blocking, // 阻塞模式
    Detached, // 分离模式，异步执行，内核不会等待它完成，适合长时间运行的任务，例如工具调用、外部事件监听等
}
#[derive(PartialEq, Eq, Clone)]
pub enum InstanceSignal {
    Terminate,
    Interrupt,
    Approve { args: String },
}

impl AsyncIoBox {
    pub fn new(kernel_tx: mpsc::Sender<InstanceToKernelEvent>) -> Self {
        let (stdin_tx, stdin_rx) = mpsc::channel::<Vec<Unit>>(1);
        let (signal_tx, signal_rx) = mpsc::channel::<InstanceSignal>(16);
        Self {
            instance: AsyncIoInstance {
                uuid: Uuid::now_v7().to_string(),
                role: AsyncIoInstanceRole::Unknown,
                execution_mode: AsyncIoInstanceExecutionMode::Blocking, // 默认阻塞模式

                stdin: stdin_rx,
                kernel_tx,
                signal_rx,

                metadata: HashMap::new(),
            },
            handle: AsyncIoHandle {
                stdin: stdin_tx,
                signal_tx,
            },
        }
    }
    pub fn with_detach_mode(mut self) -> Self {
        self.instance.execution_mode = AsyncIoInstanceExecutionMode::Detached;
        self
    }
    pub fn with_role(mut self, role: AsyncIoInstanceRole) -> Self {
        self.instance.role = role;
        self
    }
    pub fn done(self) -> Result<Self> {
        if self.instance.role == AsyncIoInstanceRole::Unknown {
            anyhow::bail!("Role must be specified");
        }
        Ok(self)
    }

    pub fn split(self) -> (AsyncIoInstance, AsyncIoHandle) {
        (self.instance, self.handle)
    }
}
