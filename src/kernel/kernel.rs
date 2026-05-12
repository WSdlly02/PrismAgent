use crate::model::app::App;
use crate::model::asyncioinstance::{AsyncIoBox, AsyncIoHandle, IoError, IoOutput};
use crate::model::event::{
    KernelError, KernelOutput, KernelStatus, KernelToShellEvent, KernelToShellPayload, KernelView,
    ShellToKernelEvent, StatusLevel, UserCommand, UserInput,
};
use crate::model::kernel::{Kernel, KernelRuntime};
use crate::model::run::Run;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use tokio::sync::mpsc;

impl Kernel {
    /// 新建Kernel，不具有运行时
    pub fn new() -> Result<Self> {
        Ok(Self {
            app: App::new().map_err(|e| anyhow!("Failed to initialize App: {e}"))?,
            runtime: None,
        })
    }

    pub fn initialize_runtime(&mut self, run_uuid: &str) -> Result<()> {
        if let Some(runtime) = &self.runtime {
            if runtime.current_run.run_metadata.uuid == run_uuid {
                return Ok(());
            }
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

    fn replace_runtime(&mut self, current_run: Run) -> Result<()> {
        let previous_runtime = self.runtime.take();
        if let Some(previous_runtime) = previous_runtime {
            if let Err(error) = self
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
        }
        self.runtime = Some(KernelRuntime {
            current_run,
            handles: HashMap::new(),
        });
        Ok(())
    }

    pub fn release_current_run_lock(&mut self) -> Result<()> {
        if let Some(runtime) = self.runtime.take() {
            self.app
                .workspace
                .release_run_lock(&runtime.current_run.run_metadata.uuid)?;
        }
        Ok(())
    }

    /// 启动内核事件循环，返回与 TUI 通信的两端
    pub fn run(
        self,
    ) -> (
        mpsc::Sender<ShellToKernelEvent>,
        mpsc::Receiver<KernelToShellEvent>,
    ) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellToKernelEvent>(64);
        let (kernel_tx, kernel_rx) = mpsc::channel::<KernelToShellEvent>(64);

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
                ($correlation_uuid:expr, $level:expr, $run_uuid:expr, $agent_uuid:expr, $message:expr) => {
                    emit!(
                        $correlation_uuid,
                        KernelToShellPayload::Status(KernelStatus {
                            level: $level,
                            run_uuid: $run_uuid,
                            agent_uuid: $agent_uuid,
                            asyncioinstance_uuid: None,
                            message: $message,
                        })
                    );
                };
            }

            macro_rules! emit_error {
                ($correlation_uuid:expr, $run_uuid:expr, $agent_uuid:expr, $message:expr) => {
                    emit!(
                        $correlation_uuid,
                        KernelToShellPayload::Error(KernelError {
                            asyncioinstance_uuid: None,
                            run_uuid: $run_uuid,
                            agent_uuid: $agent_uuid,
                            error: IoError {
                                error: $message,
                                details: String::new(),
                            },
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
                                    runtime.current_run.run_metadata.root_agent.clone()
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
                    ShellToKernelEvent::Command(request) => {
                        let correlation_uuid = Some(request.request_uuid.clone());
                        match request.command {
                            UserCommand::NewRun { title } => {
                                let title = title.unwrap_or_else(|| "Untitled run".to_string());
                                match kernel.initialize_runtime_with_new_run(&title) {
                                    Ok(()) => {
                                        let runtime =
                                            kernel.runtime.as_ref().expect("runtime exists");
                                        let run_uuid =
                                            runtime.current_run.run_metadata.uuid.clone();
                                        let agent_uuid =
                                            runtime.current_run.run_metadata.root_agent.clone();
                                        emit_status!(
                                            correlation_uuid.clone(),
                                            StatusLevel::Info,
                                            Some(run_uuid.clone()),
                                            Some(agent_uuid),
                                            format!(
                                                "New run created and resumed: {run_uuid} ({title})"
                                            )
                                        );
                                        emit_current_context!(correlation_uuid);
                                    }
                                    Err(e) => {
                                        emit_error!(
                                            correlation_uuid,
                                            None,
                                            None,
                                            format!("Failed to create and resume new run: {e}")
                                        );
                                    }
                                }
                            }
                            UserCommand::ResumeRun { run_uuid } => {
                                match kernel.initialize_runtime(&run_uuid) {
                                    Ok(()) => {
                                        let runtime =
                                            kernel.runtime.as_ref().expect("runtime exists");
                                        let run_uuid =
                                            runtime.current_run.run_metadata.uuid.clone();
                                        let agent_uuid =
                                            runtime.current_run.run_metadata.root_agent.clone();
                                        emit_status!(
                                            correlation_uuid.clone(),
                                            StatusLevel::Info,
                                            Some(run_uuid.clone()),
                                            Some(agent_uuid),
                                            format!(
                                                "Run resumed: {} ({})",
                                                run_uuid, runtime.current_run.run_metadata.title
                                            )
                                        );
                                        emit_current_context!(correlation_uuid);
                                    }
                                    Err(e) => {
                                        emit_error!(
                                            correlation_uuid,
                                            Some(run_uuid),
                                            None,
                                            format!("Failed to resume run: {e}")
                                        );
                                    }
                                }
                            }
                            UserCommand::DeleteRun { run_uuid } => {
                                emit_status!(
                                    correlation_uuid,
                                    StatusLevel::Warn,
                                    Some(run_uuid),
                                    None,
                                    "Delete run is not implemented yet.".to_string()
                                );
                            }
                            UserCommand::ListRuns => match kernel.app.workspace.list_runs() {
                                Ok(runs) => {
                                    emit!(
                                        correlation_uuid,
                                        KernelToShellPayload::View(KernelView::Runs { runs })
                                    );
                                }
                                Err(e) => {
                                    emit_error!(
                                        correlation_uuid,
                                        None,
                                        None,
                                        format!("Failed to list runs: {e}")
                                    );
                                }
                            },
                            UserCommand::FetchCurrentContext => {
                                emit_current_context!(correlation_uuid);
                            }
                            UserCommand::Cancel { .. } => {
                                emit_status!(
                                    correlation_uuid,
                                    StatusLevel::Warn,
                                    None,
                                    None,
                                    "Cancel is not implemented yet.".to_string()
                                );
                            }
                            UserCommand::Snapshot { .. } => {
                                emit_status!(
                                    correlation_uuid,
                                    StatusLevel::Warn,
                                    None,
                                    None,
                                    "Snapshot is not implemented yet.".to_string()
                                );
                            }
                            UserCommand::Shutdown => {
                                match kernel.release_current_run_lock() {
                                    Ok(()) => {
                                        emit_status!(
                                            correlation_uuid,
                                            StatusLevel::Info,
                                            None,
                                            None,
                                            "Kernel shutdown.".to_string()
                                        );
                                    }
                                    Err(e) => {
                                        emit_error!(
                                            correlation_uuid,
                                            None,
                                            None,
                                            format!("Failed to release run lock: {e}")
                                        );
                                    }
                                }
                                break;
                            }
                        }
                    }
                    ShellToKernelEvent::Input(input) => {
                        let Some(runtime) = &kernel.runtime else {
                            emit_error!(
                                Some(input.request_uuid),
                                None,
                                None,
                                "No active run; use /new <title> or /resume <run-uuid> first."
                                    .to_string()
                            );
                            continue;
                        };
                        let run_uuid = runtime.current_run.run_metadata.uuid.clone();
                        let agent_uuid = runtime.current_run.run_metadata.root_agent.clone();
                        // Process UserInput
                        // process_input(input.clone());
                        emit!(
                            Some(input.request_uuid),
                            KernelToShellPayload::Output(KernelOutput {
                                asyncioinstance_uuid: None,
                                run_uuid: Some(run_uuid),
                                agent_uuid: Some(agent_uuid),
                                content: IoOutput {
                                    streaming: false,
                                    content: format!("kernel received: {}", input.content),
                                    final_content: None,
                                },
                            })
                        );
                    }
                }
            }
        });

        (shell_tx, kernel_rx)
    }
}

pub fn process_input(input: UserInput) -> Result<AsyncIoHandle> {
    // resolve input
    let (instance, handle) = AsyncIoBox::new().done()?.split();
    //tokio::spawn(move AsyncIoInstanceProcessor)
    todo!()
}
