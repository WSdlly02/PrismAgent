use crate::model::unit::Unit;
use anyhow::Result;
use async_trait::async_trait;
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
    pub stdin: mpsc::Receiver<Vec<Unit>>,  // 标准输入-接收方
    pub stdout: mpsc::Sender<Vec<Unit>>,   // 标准输出-发送方
    pub stderr: mpsc::Sender<Vec<Unit>>,   // 标准错误-发送方
    pub signal_in: mpsc::Receiver<Signal>, // 双向通信的信号通道
    pub signal_out: mpsc::Sender<Signal>,

    pub role: AsyncIoInstanceRole,   // 实例的角色，决定了它的行为和权限
    pub data_transmit_interval: u64, // 数据传输间隔，单位毫秒，默认100ms
    pub metadata: HashMap<String, String>, // 实例的元数据, 可以存储任意键值对，供内核和Agent使用
}
/// 异步IO的控制句柄，提供给内核使用
pub struct AsyncIoHandle {
    pub stdin: mpsc::Sender<Vec<Unit>>,    // 标准输入-发送方
    pub stdout: mpsc::Receiver<Vec<Unit>>, // 标准输出-接收方
    pub stderr: mpsc::Receiver<Vec<Unit>>, // 标准错误-接收方
    pub signal_in: mpsc::Sender<Signal>,   // 双向通信的信号通道
    pub signal_out: mpsc::Receiver<Signal>,
}
#[derive(PartialEq, Eq)]
pub enum AsyncIoInstanceRole {
    Unknown,
    LLM,  // 收到Vec<Unit>后会被解析成LLM输入，输出会被解析成LLM输出
    Tool, // 收到Vec<Unit>后会判断尾部Unit是否批准、解析工具输入，输出会被解析成工具输出
}
pub struct Signal {
    pub status: SignalStatus,
    pub details: String,
}
#[derive(PartialEq, Eq)]
pub enum SignalStatus {
    Waiting,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl AsyncIoBox {
    pub fn new() -> Self {
        let (stdin_tx, stdin_rx) = mpsc::channel::<Vec<Unit>>(1);
        let (stdout_tx, stdout_rx) = mpsc::channel::<Vec<Unit>>(64);
        let (stderr_tx, stderr_rx) = mpsc::channel::<Vec<Unit>>(16);
        let (signal_in_tx, signal_in_rx) = mpsc::channel::<Signal>(16);
        let (signal_out_tx, signal_out_rx) = mpsc::channel::<Signal>(16);
        Self {
            instance: AsyncIoInstance {
                uuid: Uuid::now_v7().to_string(),
                stdin: stdin_rx,
                stdout: stdout_tx,
                stderr: stderr_tx,
                signal_in: signal_in_rx,
                signal_out: signal_out_tx,

                role: AsyncIoInstanceRole::Unknown,
                data_transmit_interval: 100,
                metadata: HashMap::new(),
            },

            handle: AsyncIoHandle {
                stdin: stdin_tx,
                stdout: stdout_rx,
                stderr: stderr_rx,
                signal_in: signal_in_tx,
                signal_out: signal_out_rx,
            },
        }
    }
    pub fn with_data_transmit_interval(mut self, interval: u64) -> Self {
        // Implementation for setting the data transmit interval
        self.instance.data_transmit_interval = interval;
        self
    }
    pub fn with_role(mut self, role: AsyncIoInstanceRole) -> Self {
        // Implementation for setting the role
        self.instance.role = role;
        self
    }
    pub fn done(self) -> Result<Self> {
        // Finalize the instance and return it
        if self.instance.role == AsyncIoInstanceRole::Unknown {
            anyhow::bail!("Role must be specified");
        }
        if self.instance.data_transmit_interval <= 50 {
            anyhow::bail!("Data transmit interval must be larger than 50ms");
        }
        Ok(self)
    }

    // 最后提供一个方法来获取控制句柄
    pub fn split(self) -> (AsyncIoInstance, AsyncIoHandle) {
        (self.instance, self.handle)
    }
}

// 定义一个异步IO实例的驱动器接口，不同的实现可以驱动不同类型的实例，例如LLM实例、工具实例等。
#[async_trait]
pub trait AsyncIoInstanceProcessor: Send + Sync {
    async fn process(&mut self, instance: AsyncIoInstance) -> Result<()>;
}
