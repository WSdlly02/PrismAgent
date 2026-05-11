use crate::model::asyncioinstance::{IoError, IoOutput};
use crate::model::run::RunSummary;

/// Shell -> Kernel 的事件。
///
/// 这里刻意保持极简：
///
/// - `Input`：用户输入的一行文本。Kernel 根据当前 input sink 决定把它送给谁：
///   - main agent
///   - 正在等待输入的 HumanInputInstance
///   - 其他阻塞等待 stdin 的 AsyncIoInstance
///
/// - `Command`：真正属于 Kernel 管理面的命令，例如创建 run、恢复 run、取消任务、关闭 kernel。
///
/// 注意：
///
/// 不再设计 `UserApprove`、`UserDecision`、`ApprovalResponse` 这类事件。
/// 用户审批、澄清、选择题都应该复用 AsyncIoInstance：
///
/// - stdin 传入提示内容
/// - stdout 表示用户批准 / 正常回答
/// - stderr 表示拒绝 / 取消 / 超时 / 错误
pub enum ShellToKernelEvent {
    /// 普通输入。
    ///
    /// Shell/TUI 不需要知道这行输入应该给谁。
    /// Kernel 根据当前 input sink 路由。
    Input(UserInput),

    /// Kernel 管理命令。
    ///
    /// 只保留无法自然建模为 AsyncIoInstance stdin/stdout/stderr 的控制操作。
    Command(UserCommandRequest),
}

/// 用户输入。
///
/// 这不是“发给 main agent 的消息”，而是“用户给 Kernel 的一行输入”。
/// 具体路由目标由 Kernel 内部的 input sink 决定。
pub struct UserInput {
    /// 请求 ID。
    ///
    /// 用于日志、调试、必要时和 Kernel 输出做关联。
    pub request_uuid: String,

    /// 用户输入的原始内容。
    pub content: String,
}

/// Shell/TUI 发给 Kernel 的控制命令请求。
///
/// 与 `UserInput` 一样，控制命令也带 request_uuid。
/// Kernel 后续发布 View / Status / Error 时，可以通过 correlation_uuid
/// 与这个请求建立弱关联；这不是 RPC，只是事件流上的可追踪性。
pub struct UserCommandRequest {
    /// 请求 ID。
    pub request_uuid: String,

    /// 实际控制命令。
    pub command: UserCommand,
}

/// Shell/TUI 发给 Kernel 的控制命令。
///
/// 这里只放真正属于 Kernel 生命周期、Run 管理、视图查询的动作。
///
/// 不要把“用户批准工具调用”放在这里；
/// 那应该由 HumanInputInstance 通过 stdout/stderr 表达。
pub enum UserCommand {
    /// 创建一个新的 run。
    NewRun { title: Option<String> },

    /// 恢复一个已有 run。
    ResumeRun { run_uuid: String },

    /// 删除一个 run。
    DeleteRun { run_uuid: String },

    /// 列出所有 run。
    ListRuns,

    /// 获取当前上下文视图。
    ///
    /// 例如当前 run、当前 agent、当前 unit_chain 摘要等。
    FetchCurrentContext,

    /// 取消当前或指定的执行对象。
    ///
    /// MVP 可以先只支持全部为 None，表示取消当前活动 asyncioinstance。
    /// 后续再按 run_uuid / agent_uuid / asyncioinstance_uuid 精准取消。
    Cancel {
        run_uuid: Option<String>,
        agent_uuid: Option<String>,
        asyncioinstance_uuid: Option<String>,
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
/// 这一侧也保持极简：
///
/// - `Output`：某个 AsyncIoInstance 的 stdout
/// - `Error`：某个 AsyncIoInstance 的 stderr
/// - `Status`：Kernel 状态提示
/// - `View`：查询类视图结果，例如 run 列表、当前上下文
///
/// Shell/TUI 只负责渲染，不拥有真实状态。
pub struct KernelToShellEvent {
    /// 与 ShellToKernelEvent 的 request_uuid 对应。
    ///
    /// 异步输出、后台状态变化等没有直接来源请求的事件可以为 None。
    pub correlation_uuid: Option<String>,

    pub payload: KernelToShellPayload,
}

pub enum KernelToShellPayload {
    /// stdout 输出。
    Output(KernelOutput),

    /// stderr 输出。
    Error(KernelError),

    /// Kernel 状态变化或提示。
    Status(KernelStatus),

    /// 查询结果 / 视图数据。
    View(KernelView),
}

/// 某个 AsyncIoInstance 的 stdout。
///
/// LLM、Tool、HumanInput、SubAgent 都可以通过这个事件把 stdout 交给 Shell/TUI 显示。
pub struct KernelOutput {
    /// 产生输出的 asyncioinstance ID。
    pub asyncioinstance_uuid: Option<String>,

    /// 所属 run。
    pub run_uuid: Option<String>,

    /// 所属 agent。
    pub agent_uuid: Option<String>,

    /// 实际输出内容。
    pub content: IoOutput,
}

/// 某个 AsyncIoInstance 的 stderr。
///
/// 工具失败、用户拒绝审批、超时、LLM 调用失败等，都可以走 stderr。
pub struct KernelError {
    /// 产生错误的 instance ID。
    pub asyncioinstance_uuid: Option<String>,

    /// 所属 run。
    pub run_uuid: Option<String>,

    /// 所属 agent。
    pub agent_uuid: Option<String>,

    /// 实际错误内容。
    pub error: IoError,
}

/// Kernel 状态提示。
///
/// 这不是严格业务状态机，只是给 Shell/TUI 展示状态变化。
/// 例如：
///
/// - 已创建 run
/// - 已恢复 run
/// - 正在运行 LLM
/// - 已取消任务
/// - Kernel 即将关闭

pub struct KernelStatus {
    /// 状态级别。
    pub level: StatusLevel,

    /// 可选作用域：run。
    pub run_uuid: Option<String>,

    /// 可选作用域：agent。
    pub agent_uuid: Option<String>,

    /// 可选作用域：asyncioinstance。
    pub asyncioinstance_uuid: Option<String>,

    /// 给用户看的简短状态文本。
    pub message: String,
}

/// 状态级别。
#[derive(PartialEq)]
pub enum StatusLevel {
    Info,
    Warn,
    Error,
}

/// Kernel 返回给 Shell/TUI 的视图数据。
///
/// 视图类事件用于回答：
///
/// - ListRuns
/// - FetchCurrentContext
/// - Snapshot
///
/// 它们不是“Response”，只是 Kernel 发布的 View。
pub enum KernelView {
    /// run 列表。
    Runs { runs: Vec<RunSummary> },

    /// 当前上下文视图。
    ///
    /// 注意：这里暂时不直接暴露完整 unit_chain，
    /// 而是先给足够 TUI 展示的信息。
    /// 后续如果需要，可以单独加 `units: Vec<Unit>` 或 `unit_ids: Vec<String>`。
    CurrentContext {
        run_uuid: Option<String>,
        agent_uuid: Option<String>,
        title: Option<String>,
        unit_count: usize,
        head_unit_uuid: Option<String>,
    },

    /// 快照已创建。
    SnapshotCreated {
        run_uuid: String,
        snapshot_uid: String,
        name: Option<String>,
    },

    /// run 已删除。
    RunDeleted { run_uuid: String },
}
