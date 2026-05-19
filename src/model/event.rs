use crate::model::agent::Agent;
use crate::model::run::RunMetadata;
use crate::model::unit::Unit;

/// Shell -> Kernel 的事件。
///
/// 这里刻意保持极简：
///
/// - `Input`：用户输入的一行文本。Shell/TUI 必须显式指定目标，
///   Kernel 负责校验目标是否存在且是否可以接收输入。
///
/// - `Command`：真正属于 Kernel 管理面的命令，例如创建 run、恢复 run、取消任务、关闭 kernel。
///
/// 注意：
///
/// 不再设计 `UserApprove`、`UserDecision`、`ApprovalResponse` 这类事件。
/// 用户审批、澄清、选择题都应该复用 AsyncIoInstance：
///
/// - stdin 传入提示内容
/// - InstanceToKernelEvent::Output 表示用户批准 / 正常回答
/// - InstanceToKernelEvent::Error 表示拒绝 / 取消 / 超时 / 错误
pub enum ShellToKernelEvent {
    /// 普通输入。
    ///
    /// Shell/TUI 必须显式指定目标。
    /// Kernel 不猜测路由，只校验与执行。
    Input(UserInput),

    /// Kernel 管理命令。
    ///
    /// 只保留无法自然建模为 AsyncIoInstance 输入输出的控制操作。
    KernelCommand(UserKernelCommandRequest),
}

/// 用户输入。
///
/// 这不是“发给当前 agent 的消息”，而是“用户给某个明确目标的一行输入”。
pub struct UserInput {
    /// 请求 ID。
    ///
    /// 用于日志、调试、必要时和 Kernel 输出做关联。
    pub request_uuid: String,

    /// 所属 run。
    pub run_uuid: String,

    /// 所属 agent。
    pub agent_uuid: String,

    /// 用户输入的原始内容。
    pub content: String,
}

/// Shell/TUI 发给 Kernel 的控制命令请求。
///
/// 与 `UserInput` 一样，控制命令也带 request_uuid。
/// Kernel 后续发布 View / Status / Error 时，可以通过 correlation_uuid
/// 与这个请求建立弱关联；这不是 RPC，只是事件流上的可追踪性。
pub struct UserKernelCommandRequest {
    /// 请求 ID。
    pub request_uuid: String,

    /// 实际控制命令。
    pub command: UserKernelCommand,
}

/// Shell/TUI 发给 Kernel 的控制命令。
///
/// 这里只放真正属于 Kernel 生命周期、Run 管理、视图查询的动作。
pub enum UserKernelCommand {
    /// 创建一个新的 run。
    NewRun { title: Option<String> },

    /// 恢复一个已有 run。
    ResumeRun { run_uuid: String },

    /// 删除一个 run。
    DeleteRun { run_uuid: String },

    /// 列出所有 run。
    ListRuns,

    /// 取消当前或指定的执行对象。
    ///
    /// Shell 只表达“取消某个 agent 的前台任务”的意图；
    /// 具体映射到哪个 AsyncIoInstance 由 Kernel 根据 handles 决定。
    Cancel {
        run_uuid: Option<String>,
        agent_uuid: Option<String>,
    },

    /// 批准某个 agent 当前挂起的工具调用。
    ///
    /// Kernel 只负责把这条控制信号转发给该 agent 的 blocking tool instance；
    /// 具体参数解析由 tool instance 完成。
    Approve {
        run_uuid: Option<String>,
        agent_uuid: Option<String>,
        args: String,
    },

    /// 关闭 Kernel，释放锁并退出。
    Shutdown,

    /// 创建快照。
    ///
    /// 如果 run_uuid 为 None，则对当前 run 快照。
    Snapshot {
        run_uuid: Option<String>,
        snapshot_uid: Option<String>,
    },
}

/// Kernel -> Shell 的事件。
///
/// Snapshot 是当前 run 的完整状态投影；Shell 只负责保存、比对和渲染。
/// Patch 是无法自然建模到当前 run snapshot 的一次性可打印文本，例如 /list。
pub enum KernelToShellEvent {
    Snapshot {
        correlation_uuid: Option<String>,
        snapshot: KernelSnapshot,
    },
    Patch {
        correlation_uuid: Option<String>,
        text: String,
    },
}

pub struct KernelSnapshot {
    pub run_metadata: RunMetadata,
    pub agents: Vec<AgentSnapshot>,
}

pub struct AgentSnapshot {
    pub agent: Agent,
    pub units: Vec<Unit>,
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
    pub units: Vec<Unit>,
    /// 是否为 LLM 产生的工具调用请求。工具调用请求仍会先 commit 到
    /// unit_chain，但随后由 kernel 拉起 tool instance 等待审批。
    pub is_tool_calls: bool,
}
