use crate::model::run::RunSummary;
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

    /// 输入目标。
    pub target: InputTarget,

    /// 用户输入的原始内容。
    pub content: String,
}

/// 用户输入目标。
///
/// Agent 表示普通对话输入；Instance 表示投递给某个正在等待 stdin 的具体
/// AsyncIoInstance，例如人工确认、选择题、审批等。
pub enum InputTarget {
    Agent { agent_uuid: String },
    Instance { asyncioinstance_uuid: String },
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
///
/// 不要把“用户批准工具调用”放在这里；
/// 那应该由 HumanInputInstance 通过实例输出表达。
pub enum UserKernelCommand {
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
/// - `Stdout`：某个 AsyncIoInstance 的普通输出
/// - `Stderr`：某个 AsyncIoInstance 的错误输出
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
    Stdout(KernelUnitStream),

    /// stderr 输出。
    Stderr(KernelUnitStream),

    /// Kernel 状态变化或提示。
    Status(KernelStatus),

    /// 查询结果 / 视图数据。
    View(KernelView),
}

/// 某个 AsyncIoInstance 的输出流。
///
/// LLM、Tool、HumanInput、SubAgent 都通过 Vec<Unit> 与 Shell/TUI 交换内容。
pub struct KernelUnitStream {
    /// 产生输出的 asyncioinstance ID。
    pub asyncioinstance_uuid: Option<String>,

    /// 所属 run。
    pub run_uuid: Option<String>,

    /// 所属 agent。
    pub agent_uuid: Option<String>,

    /// 实际 unit 链。
    pub units: Vec<Unit>,
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

    /// 机器可读运行状态。
    pub runtime_status: Option<RuntimeStatus>,

    /// 给用户看的简短状态文本。
    pub message: String,
}

/// 状态级别。
#[derive(PartialEq, Eq)]
pub enum StatusLevel {
    Info,
    Warn,
    Error,
}

/// Kernel/实例运行状态。
///
/// TUI 应根据这个字段控制输入框状态，而不是解析 message 文本。
#[derive(PartialEq, Eq)]
pub enum RuntimeStatus {
    Accepted,
    Running,
    WaitingInput,
    Done,
    Failed,
    Cancelled,
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
