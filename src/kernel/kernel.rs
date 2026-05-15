use crate::model::app::App;
use crate::model::event::{
    InputTarget, KernelStatus, KernelToShellEvent, KernelToShellPayload, KernelUnitStream,
    KernelView, RuntimeStatus, ShellToKernelEvent, StatusLevel, UserKernelCommand,
};
use crate::model::kernel::{Kernel, KernelRuntime};
use crate::model::run::Run;
use crate::model::unit::{Unit, UnitKind, UnitRole, UnitScope, UnitVisibility};
use anyhow::{Result, anyhow};
use chrono::Utc;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

impl Kernel {
    /// 新建 Kernel，不具有运行时。
    pub fn new() -> Result<Self> {
        Ok(Self {
            app: App::new().map_err(|e| anyhow!("Failed to initialize App: {e}"))?,
            runtime: None,
        })
    }

    pub fn initialize_runtime(&mut self, run_uuid: &str) -> Result<()> {
        if let Some(runtime) = &self.runtime
            && runtime.current_run.run_metadata.uuid == run_uuid
        {
            return Ok(());
        }
        let current_run = self.app.workspace.resume_run(run_uuid)?;
        self.replace_runtime(current_run)?;
        Ok(())
    }

    pub fn initialize_runtime_with_new_run(&mut self, title: &str) -> Result<()> {
        let current_run = self.app.workspace.create_run_and_resume(title)?;
        self.replace_runtime(current_run)?;
        Ok(())
    }

    /// 替换当前运行时，确保释放之前运行时的锁
    fn replace_runtime(&mut self, current_run: Run) -> Result<()> {
        let previous_runtime = self.runtime.take();
        if let Some(previous_runtime) = previous_runtime
            && let Err(error) = self
                .app
                .workspace
                .release_run_lock(&previous_runtime.current_run.run_metadata.uuid)
        {
            let _ = self
                .app
                .workspace
                .release_run_lock(&current_run.run_metadata.uuid);
            self.runtime = Some(previous_runtime);
            return Err(error);
        }

        self.runtime = Some(KernelRuntime {
            current_run,
            handles: HashMap::new(),
        });
        Ok(())
    }

    /// 释放当前运行时的锁，通常在内核关闭前调用
    pub fn release_current_run_lock(&mut self) -> Result<()> {
        if let Some(runtime) = self.runtime.take() {
            self.app
                .workspace
                .release_run_lock(&runtime.current_run.run_metadata.uuid)?;
        }
        Ok(())
    }

    /// 启动内核事件循环，返回与 TUI 通信的两端。
    pub fn run(
        self,
    ) -> (
        mpsc::Sender<ShellToKernelEvent>,
        mpsc::Receiver<KernelToShellEvent>,
    ) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellToKernelEvent>(64);
        let (kernel_tx, kernel_rx) = mpsc::channel::<KernelToShellEvent>(64);
        let (egress_tx, mut egress_rx) = mpsc::channel::<KernelToShellEvent>(256);

        // 生成异步的消息发送器，将内核事件发送到 TUI
        tokio::spawn(async move {
            while let Some(event) = egress_rx.recv().await {
                if kernel_tx.send(event).await.is_err() {
                    break;
                }
            }
        });

        // 生成异步的消息接收器，处理来自 TUI 的事件
        // ShellToKernelEvent::Command => 内联处理逻辑，将结果通过 egress_tx 发送状态和视图事件，egress_tx 事件最终会被上面的发送器转发给 TUI
        // ShellToKernelEvent::Input => 生成一个新的异步任务处理输入，并通过 egress_tx 发送状态和输出事件，egress_tx 事件最终会被上面的发送器转发给 TUI
        tokio::spawn(async move {
            let mut kernel = self;

            macro_rules! emit {
                ($correlation_uuid:expr, $payload:expr) => {
                    if egress_tx
                        .send(KernelToShellEvent {
                            correlation_uuid: $correlation_uuid,
                            payload: $payload,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                };
            }

            macro_rules! emit_status {
                ($correlation_uuid:expr, $level:expr, $runtime_status:expr, $run_uuid:expr, $agent_uuid:expr, $instance_uuid:expr, $message:expr) => {
                    emit!(
                        $correlation_uuid,
                        KernelToShellPayload::Status(KernelStatus {
                            level: $level,
                            run_uuid: $run_uuid,
                            agent_uuid: $agent_uuid,
                            asyncioinstance_uuid: $instance_uuid,
                            runtime_status: $runtime_status,
                            message: $message,
                        })
                    );
                };
            }

            macro_rules! emit_stderr {
                ($correlation_uuid:expr, $run_uuid:expr, $agent_uuid:expr, $instance_uuid:expr, $message:expr) => {
                    emit!(
                        $correlation_uuid,
                        KernelToShellPayload::Stderr(KernelUnitStream {
                            asyncioinstance_uuid: $instance_uuid,
                            run_uuid: $run_uuid,
                            agent_uuid: $agent_uuid,
                            units: vec![unit_with_message(
                                UnitKind::GenericResult,
                                UnitRole::System,
                                UnitVisibility::Public,
                                $message,
                            )],
                        })
                    );
                };
            }

            macro_rules! emit_current_context {
                ($correlation_uuid:expr) => {
                    if let Some(runtime) = &kernel.runtime {
                        emit!(
                            $correlation_uuid,
                            KernelToShellPayload::View(KernelView::CurrentContext {
                                run_uuid: Some(runtime.current_run.run_metadata.uuid.clone()),
                                agent_uuid: Some(
                                    runtime.current_run.run_metadata.root_agent_uuid.clone()
                                ),
                                title: Some(runtime.current_run.run_metadata.title.clone()),
                                unit_count: 0,
                                head_unit_uuid: None,
                            })
                        );
                    } else {
                        emit!(
                            $correlation_uuid,
                            KernelToShellPayload::View(KernelView::CurrentContext {
                                run_uuid: None,
                                agent_uuid: None,
                                title: None,
                                unit_count: 0,
                                head_unit_uuid: None,
                            })
                        );
                    }
                };
            }

            while let Some(event) = shell_rx.recv().await {
                match event {
                    ShellToKernelEvent::ShellCommand(request) => {
                        let correlation_uuid = Some(Uuid::now_v7().to_string());
                        emit_status!(
                            correlation_uuid,
                            StatusLevel::Warn,
                            None,
                            None,
                            None,
                            None,
                            format!(
                                "Shell command execution is not implemented yet: {}",
                                request.command
                            )
                        );
                    }
                    ShellToKernelEvent::KernelCommand(request) => {
                        let correlation_uuid = Some(request.request_uuid.clone());
                        match request.command {
                            UserKernelCommand::NewRun { title } => {
                                let title = title.unwrap_or_else(|| "Untitled run".to_string());
                                match kernel.initialize_runtime_with_new_run(&title) {
                                    Ok(()) => {
                                        let runtime =
                                            kernel.runtime.as_ref().expect("runtime exists");
                                        let run_uuid =
                                            runtime.current_run.run_metadata.uuid.clone();
                                        let agent_uuid = runtime
                                            .current_run
                                            .run_metadata
                                            .root_agent_uuid
                                            .clone();
                                        emit_status!(
                                            correlation_uuid.clone(),
                                            StatusLevel::Info,
                                            None,
                                            Some(run_uuid.clone()),
                                            Some(agent_uuid),
                                            None,
                                            format!(
                                                "New run created and resumed: {run_uuid} ({title})"
                                            )
                                        );
                                        emit_current_context!(correlation_uuid);
                                    }
                                    Err(error) => {
                                        emit_stderr!(
                                            correlation_uuid,
                                            None,
                                            None,
                                            None,
                                            format!("Failed to create and resume new run: {error}")
                                        );
                                    }
                                }
                            }
                            UserKernelCommand::ResumeRun { run_uuid } => {
                                match kernel.initialize_runtime(&run_uuid) {
                                    Ok(()) => {
                                        let runtime =
                                            kernel.runtime.as_ref().expect("runtime exists");
                                        let run_uuid =
                                            runtime.current_run.run_metadata.uuid.clone();
                                        let agent_uuid = runtime
                                            .current_run
                                            .run_metadata
                                            .root_agent_uuid
                                            .clone();
                                        emit_status!(
                                            correlation_uuid.clone(),
                                            StatusLevel::Info,
                                            None,
                                            Some(run_uuid.clone()),
                                            Some(agent_uuid),
                                            None,
                                            format!(
                                                "Run resumed: {} ({})",
                                                run_uuid, runtime.current_run.run_metadata.title
                                            )
                                        );
                                        emit_current_context!(correlation_uuid);
                                    }
                                    Err(error) => {
                                        emit_stderr!(
                                            correlation_uuid,
                                            Some(run_uuid),
                                            None,
                                            None,
                                            format!("Failed to resume run: {error}")
                                        );
                                    }
                                }
                            }
                            UserKernelCommand::DeleteRun { run_uuid } => {
                                emit_status!(
                                    correlation_uuid,
                                    StatusLevel::Warn,
                                    None,
                                    Some(run_uuid),
                                    None,
                                    None,
                                    "Delete run is not implemented yet.".to_string()
                                );
                            }
                            UserKernelCommand::ListRuns => match kernel.app.workspace.list_runs() {
                                Ok(runs) => {
                                    emit!(
                                        correlation_uuid,
                                        KernelToShellPayload::View(KernelView::Runs { runs })
                                    );
                                }
                                Err(error) => {
                                    emit_stderr!(
                                        correlation_uuid,
                                        None,
                                        None,
                                        None,
                                        format!("Failed to list runs: {error}")
                                    );
                                }
                            },
                            UserKernelCommand::FetchCurrentContext => {
                                emit_current_context!(correlation_uuid);
                            }
                            UserKernelCommand::Cancel { .. } => {
                                emit_status!(
                                    correlation_uuid,
                                    StatusLevel::Warn,
                                    Some(RuntimeStatus::Cancelled),
                                    None,
                                    None,
                                    None,
                                    "Cancel is not implemented yet.".to_string()
                                );
                            }
                            UserKernelCommand::Snapshot { .. } => {
                                emit_status!(
                                    correlation_uuid,
                                    StatusLevel::Warn,
                                    None,
                                    None,
                                    None,
                                    None,
                                    "Snapshot is not implemented yet.".to_string()
                                );
                            }
                            UserKernelCommand::Shutdown => {
                                match kernel.release_current_run_lock() {
                                    Ok(()) => {
                                        emit_status!(
                                            correlation_uuid,
                                            StatusLevel::Info,
                                            Some(RuntimeStatus::Done),
                                            None,
                                            None,
                                            None,
                                            "Kernel shutdown.".to_string()
                                        );
                                    }
                                    Err(error) => {
                                        emit_stderr!(
                                            correlation_uuid,
                                            None,
                                            None,
                                            None,
                                            format!("Failed to release run lock: {error}")
                                        );
                                    }
                                }
                                break;
                            }
                        }
                    }
                    ShellToKernelEvent::Input(input) => {
                        // 阻塞校验输入的 run_uuid 和 agent_uuid 是否与当前 runtime 匹配
                        let Some(runtime) = &kernel.runtime else {
                            emit_stderr!(
                                Some(input.request_uuid),
                                None,
                                None,
                                None,
                                "No active run; use /new <title> or /resume <run-uuid> first."
                                    .to_string()
                            );
                            continue;
                        };

                        let run_uuid = runtime.current_run.run_metadata.uuid.clone();
                        let agent_uuid = match input.target {
                            InputTarget::Agent { agent_uuid } => agent_uuid,
                            InputTarget::Instance {
                                asyncioinstance_uuid,
                            } => {
                                emit_status!(
                                    Some(input.request_uuid),
                                    StatusLevel::Warn,
                                    Some(RuntimeStatus::WaitingInput),
                                    Some(run_uuid),
                                    None,
                                    Some(asyncioinstance_uuid),
                                    "Instance input routing is not implemented yet.".to_string()
                                );
                                continue;
                            }
                        };

                        // 目前仅支持发送给 root agent 的输入，后续会根据 agent_uuid 查找对应的 agent 和 unit 来路由输入
                        if agent_uuid != runtime.current_run.run_metadata.root_agent_uuid {
                            emit_stderr!(
                                Some(input.request_uuid),
                                Some(run_uuid),
                                Some(agent_uuid),
                                None,
                                "Target agent is not available in current run.".to_string()
                            );
                            continue;
                        }

                        let instance_uuid = Uuid::now_v7().to_string();
                        emit_status!(
                            Some(input.request_uuid.clone()),
                            StatusLevel::Info,
                            Some(RuntimeStatus::Accepted),
                            Some(run_uuid.clone()),
                            Some(agent_uuid.clone()),
                            Some(instance_uuid.clone()),
                            "Input accepted.".to_string()
                        );

                        spawn_input_instance(
                            egress_tx.clone(),
                            input.request_uuid,
                            run_uuid,
                            agent_uuid,
                            instance_uuid,
                            input.content,
                        );
                    }
                }
            }
        });

        (shell_tx, kernel_rx)
    }
}

fn spawn_input_instance(
    egress_tx: mpsc::Sender<KernelToShellEvent>,
    request_uuid: String,
    run_uuid: String,
    agent_uuid: String,
    instance_uuid: String,
    content: String,
) {
    tokio::spawn(async move {
        macro_rules! send_or_return {
            ($payload:expr) => {
                if egress_tx
                    .send(KernelToShellEvent {
                        correlation_uuid: Some(request_uuid.clone()),
                        payload: $payload,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            };
        }

        send_or_return!(KernelToShellPayload::Status(KernelStatus {
            level: StatusLevel::Info,
            run_uuid: Some(run_uuid.clone()),
            agent_uuid: Some(agent_uuid.clone()),
            asyncioinstance_uuid: Some(instance_uuid.clone()),
            runtime_status: Some(RuntimeStatus::Running),
            message: "Input processing started.".to_string(),
        }));

        send_or_return!(KernelToShellPayload::Stdout(KernelUnitStream {
            asyncioinstance_uuid: Some(instance_uuid.clone()),
            run_uuid: Some(run_uuid.clone()),
            agent_uuid: Some(agent_uuid.clone()),
            units: vec![unit_with_message(
                UnitKind::GenericResult,
                UnitRole::Assistant,
                UnitVisibility::Public,
                format!("kernel received: {content}"),
            )],
        }));

        send_or_return!(KernelToShellPayload::Status(KernelStatus {
            level: StatusLevel::Info,
            run_uuid: Some(run_uuid),
            agent_uuid: Some(agent_uuid),
            asyncioinstance_uuid: Some(instance_uuid),
            runtime_status: Some(RuntimeStatus::Done),
            message: "Input processing finished.".to_string(),
        }));
    });
}

fn unit_with_message(
    kind: UnitKind,
    role: UnitRole,
    visibility: UnitVisibility,
    message: String,
) -> Unit {
    Unit {
        uuid: Uuid::now_v7().to_string(),
        atom_hash: "0".repeat(64),
        kind,
        role,
        scope: UnitScope::Agent,
        visibility,
        metadata: HashMap::from([("content".to_string(), message)]),
        created_at: Utc::now().timestamp(),
    }
}
