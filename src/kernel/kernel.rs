use crate::kernel::pipeline::{input_pipeline, output_pipeline, unit_with_content};
use crate::model::app::App;
use crate::model::asyncioinstance::{
    AsyncIoBox, AsyncIoInstance, AsyncIoInstanceRole, InstanceSignal,
};
use crate::model::event::{
    InputTarget, KernelStatus, KernelToShellEvent, KernelToShellPayload, KernelUnitStream,
    KernelView, RuntimeStatus, ShellToKernelEvent, StatusLevel, UserKernelCommand,
};
use crate::model::kernel::{InstanceStream, InstanceToKernelEvent, Kernel, KernelRuntime};
use crate::model::run::Run;
use crate::model::unit::{UnitKind, UnitRole, UnitVisibility};
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use tokio::sync::mpsc;

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
        let (instance_tx, mut instance_rx) = mpsc::channel::<InstanceToKernelEvent>(256);

        // Kernel runtime 只有这个任务持有。Shell 事件和 instance 输出通过 select!
        // 串行进入这里，避免 Arc<Mutex<KernelRuntime>>。
        tokio::spawn(async move {
            let mut kernel = self;

            macro_rules! emit {
                ($correlation_uuid:expr, $payload:expr) => {
                    if kernel_tx
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
                            units: vec![unit_with_content(
                                UnitKind::GenericResult,
                                UnitRole::System,
                                UnitVisibility::Public,
                                None,
                                $message,
                                HashMap::new(),
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

            loop {
                tokio::select! {
                    shell_event = shell_rx.recv() => {
                        let Some(shell_event) = shell_event else {
                            break;
                        };

                        match shell_event {
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
                        let Some(runtime) = kernel.runtime.as_mut() else {
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

                        let input_units = match input_pipeline(
                            &runtime.current_run,
                            &agent_uuid,
                            &input.request_uuid,
                            &input.content,
                        ) {
                            Ok(units) => units,
                            Err(error) => {
                                emit_stderr!(
                                    Some(input.request_uuid),
                                    Some(run_uuid),
                                    Some(agent_uuid),
                                    None,
                                    format!("Input pipeline failed: {error}")
                                );
                                continue;
                            }
                        };

                        let io_box = match AsyncIoBox::new(instance_tx.clone())
                            .with_role(AsyncIoInstanceRole::LLM)
                            .done()
                        {
                            Ok(io_box) => io_box,
                            Err(error) => {
                                emit_stderr!(
                                    Some(input.request_uuid),
                                    Some(run_uuid),
                                    Some(agent_uuid),
                                    None,
                                    format!("Failed to create AsyncIoInstance: {error}")
                                );
                                continue;
                            }
                        };
                        let (instance, handle) = io_box.split();
                        let instance_uuid = instance.uuid.clone();
                        if let Err(error) = handle.stdin.try_send(input_units) {
                            emit_stderr!(
                                Some(input.request_uuid),
                                Some(run_uuid),
                                Some(agent_uuid),
                                Some(instance_uuid),
                                format!("Failed to send input to AsyncIoInstance: {error}")
                            );
                            continue;
                        }
                        runtime.handles.insert(instance_uuid.clone(), handle);

                        emit_status!(
                            Some(input.request_uuid.clone()),
                            StatusLevel::Info,
                            Some(RuntimeStatus::Running),
                            Some(run_uuid.clone()),
                            Some(agent_uuid.clone()),
                            Some(instance_uuid.clone()),
                            "running".to_string()
                        );

                        spawn_input_instance(
                            input.request_uuid,
                            run_uuid,
                            agent_uuid,
                            instance,
                        );
                            }
                        }
                    }
                    instance_event = instance_rx.recv() => {
                        let Some(instance_event) = instance_event else {
                            continue;
                        };
                        let Some(runtime) = kernel.runtime.as_mut() else {
                            emit_stderr!(
                                instance_event.correlation_uuid,
                                Some(instance_event.run_uuid),
                                Some(instance_event.agent_uuid),
                                Some(instance_event.asyncioinstance_uuid),
                                "Instance returned output without active run.".to_string()
                            );
                            continue;
                        };

                        if runtime.current_run.run_metadata.uuid != instance_event.run_uuid {
                            runtime.handles.remove(&instance_event.asyncioinstance_uuid);
                            emit_stderr!(
                                instance_event.correlation_uuid,
                                Some(instance_event.run_uuid),
                                Some(instance_event.agent_uuid),
                                Some(instance_event.asyncioinstance_uuid),
                                "Instance output does not belong to current run.".to_string()
                            );
                            continue;
                        }

                        let (is_stderr, units) = match instance_event.stream {
                            InstanceStream::Output(units) => (false, units),
                            InstanceStream::Error(units) => (true, units),
                        };

                        let committed_units = match output_pipeline(
                            &kernel.app.workspace,
                            &mut runtime.current_run,
                            &instance_event.agent_uuid,
                            units,
                        ) {
                            Ok(units) => units,
                            Err(error) => {
                                runtime.handles.remove(&instance_event.asyncioinstance_uuid);
                                emit_stderr!(
                                    instance_event.correlation_uuid,
                                    Some(instance_event.run_uuid),
                                    Some(instance_event.agent_uuid),
                                    Some(instance_event.asyncioinstance_uuid),
                                    format!("Output pipeline failed: {error}")
                                );
                                continue;
                            }
                        };
                        runtime.handles.remove(&instance_event.asyncioinstance_uuid);

                        let payload = KernelUnitStream {
                            asyncioinstance_uuid: Some(instance_event.asyncioinstance_uuid.clone()),
                            run_uuid: Some(instance_event.run_uuid.clone()),
                            agent_uuid: Some(instance_event.agent_uuid.clone()),
                            units: committed_units,
                        };
                        if is_stderr {
                            emit!(
                                instance_event.correlation_uuid.clone(),
                                KernelToShellPayload::Stderr(payload)
                            );
                        } else {
                            emit!(
                                instance_event.correlation_uuid.clone(),
                                KernelToShellPayload::Stdout(payload)
                            );
                        }
                        emit_status!(
                            instance_event.correlation_uuid,
                            StatusLevel::Info,
                            Some(RuntimeStatus::Done),
                            Some(instance_event.run_uuid),
                            Some(instance_event.agent_uuid),
                            Some(instance_event.asyncioinstance_uuid),
                            "idle".to_string()
                        );
                    }
                }
            }
        });

        (shell_tx, kernel_rx)
    }
}

fn spawn_input_instance(
    request_uuid: String,
    run_uuid: String,
    agent_uuid: String,
    instance: AsyncIoInstance,
) {
    tokio::spawn(async move {
        let AsyncIoInstance {
            uuid: instance_uuid,
            mut stdin,
            mut signal_rx,
            kernel_tx,
            ..
        } = instance;

        let mut units = tokio::select! {
            units = stdin.recv() => {
                let Some(units) = units else {
                    send_instance_error(
                        &kernel_tx,
                        request_uuid,
                        run_uuid,
                        agent_uuid,
                        instance_uuid,
                        "AsyncIoInstance closed before receiving input.".to_string(),
                    ).await;
                    return;
                };
                units
            }
            signal = signal_rx.recv() => {
                let message = match signal {
                    Some(InstanceSignal::Terminate) => "AsyncIoInstance terminated before receiving input.",
                    Some(InstanceSignal::Interrupt) => "AsyncIoInstance interrupted before receiving input.",
                    None => "AsyncIoInstance signal channel closed before receiving input.",
                };
                send_instance_error(
                    &kernel_tx,
                    request_uuid,
                    run_uuid,
                    agent_uuid,
                    instance_uuid,
                    message.to_string(),
                ).await;
                return;
            }
        };

        let input_preview = units
            .last()
            .and_then(|unit| unit.metadata.get("content"))
            .cloned()
            .unwrap_or_default();
        units.push(unit_with_content(
            UnitKind::GenericResult,
            UnitRole::Assistant,
            UnitVisibility::Public,
            Some(&agent_uuid),
            format!("kernel received: {input_preview}"),
            HashMap::new(),
        ));
        let _ = kernel_tx
            .send(InstanceToKernelEvent {
                correlation_uuid: Some(request_uuid),
                run_uuid,
                agent_uuid,
                asyncioinstance_uuid: instance_uuid,
                stream: InstanceStream::Output(units),
            })
            .await;
    });
}

async fn send_instance_error(
    kernel_tx: &mpsc::Sender<InstanceToKernelEvent>,
    request_uuid: String,
    run_uuid: String,
    agent_uuid: String,
    instance_uuid: String,
    message: String,
) {
    let _ = kernel_tx
        .send(InstanceToKernelEvent {
            correlation_uuid: Some(request_uuid),
            run_uuid,
            agent_uuid,
            asyncioinstance_uuid: instance_uuid,
            stream: InstanceStream::Error(vec![unit_with_content(
                UnitKind::GenericResult,
                UnitRole::System,
                UnitVisibility::Public,
                None,
                message,
                HashMap::new(),
            )]),
        })
        .await;
}
