use crate::model::app::App;
use crate::model::asyncioinstance::IoOutput;
use crate::model::event::{CommandEvent, KernelToShellEvent, ShellToKernelEvent};
use crate::model::kernel::{Kernel, KernelRuntime};
use crate::model::run::Run;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use tokio::sync::mpsc;

impl Kernel {
    pub fn new() -> Result<Self> {
        Ok(Self {
            app: App::new().map_err(|e| anyhow!("Failed to initialize App: {e}"))?,
            runtime: None,
        })
    }
    pub fn initialize_runtime(&mut self, run_id: &str) -> Result<()> {
        if let Some(runtime) = &self.runtime {
            if runtime.current_run.run_metadata.uuid == run_id {
                return Ok(());
            }
        }
        let current_run = self.app.workspace.resume_run(run_id)?;
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
            macro_rules! send_kernel_event_or_break {
                ($kernel_tx:expr, $kernel_event:expr) => {
                    if $kernel_tx.send($kernel_event).await.is_err() {
                        break;
                    }
                };
            }
            while let Some(event) = shell_rx.recv().await {
                match event {
                    ShellToKernelEvent::CommandInput { command } => match command {
                        CommandEvent::NewRun { title } => {
                            match kernel.initialize_runtime_with_new_run(&title) {
                                Ok(()) => {
                                    let runtime = kernel.runtime.as_ref().expect("runtime exists");
                                    send_kernel_event_or_break!(
                                        kernel_tx,
                                        KernelToShellEvent::RunActivated {
                                            run_id: runtime.current_run.run_metadata.uuid.clone(),
                                            agent_id: runtime
                                                .current_run
                                                .run_metadata
                                                .root_agent
                                                .clone(),
                                            title: runtime.current_run.run_metadata.title.clone(),
                                        }
                                    );
                                    send_kernel_event_or_break!(
                                        kernel_tx,
                                        KernelToShellEvent::Output {
                                            run_id: runtime.current_run.run_metadata.uuid.clone(),
                                            agent_id: runtime
                                                .current_run
                                                .run_metadata
                                                .root_agent
                                                .clone(),
                                            content: IoOutput {
                                                streaming: false,
                                                content: format!(
                                                    "New run created and resumed: {} ({title})",
                                                    runtime.current_run.run_metadata.uuid
                                                ),
                                                final_content: None,
                                            },
                                        }
                                    );
                                }
                                Err(e) => {
                                    send_kernel_event_or_break!(
                                        kernel_tx,
                                        KernelToShellEvent::Output {
                                            run_id: "N/A".to_string(),
                                            agent_id: "N/A".to_string(),
                                            content: IoOutput {
                                                streaming: false,
                                                content: format!(
                                                    "Failed to create and resume new run: {e}"
                                                ),
                                                final_content: None,
                                            },
                                        }
                                    );
                                }
                            }
                        }
                        CommandEvent::ResumeRun { run_id } => {
                            match kernel.initialize_runtime(&run_id) {
                                Ok(()) => {
                                    let runtime = kernel.runtime.as_ref().expect("runtime exists");
                                    send_kernel_event_or_break!(
                                        kernel_tx,
                                        KernelToShellEvent::RunActivated {
                                            run_id: runtime.current_run.run_metadata.uuid.clone(),
                                            agent_id: runtime
                                                .current_run
                                                .run_metadata
                                                .root_agent
                                                .clone(),
                                            title: runtime.current_run.run_metadata.title.clone(),
                                        }
                                    );
                                    send_kernel_event_or_break!(
                                        kernel_tx,
                                        KernelToShellEvent::Output {
                                            run_id: runtime.current_run.run_metadata.uuid.clone(),
                                            agent_id: runtime
                                                .current_run
                                                .run_metadata
                                                .root_agent
                                                .clone(),
                                            content: IoOutput {
                                                streaming: false,
                                                content: format!(
                                                    "Run resumed: {} ({})",
                                                    runtime.current_run.run_metadata.uuid,
                                                    runtime.current_run.run_metadata.title
                                                ),
                                                final_content: None,
                                            },
                                        }
                                    );
                                }
                                Err(e) => {
                                    send_kernel_event_or_break!(
                                        kernel_tx,
                                        KernelToShellEvent::Output {
                                            run_id: run_id.clone(),
                                            agent_id: "N/A".to_string(),
                                            content: IoOutput {
                                                streaming: false,
                                                content: format!(
                                                    "Failed to resume run {run_id}: {e}"
                                                ),
                                                final_content: None,
                                            },
                                        }
                                    );
                                }
                            }
                        }
                        CommandEvent::DeleteRun { run_id } => {
                            send_kernel_event_or_break!(
                                kernel_tx,
                                KernelToShellEvent::Output {
                                    run_id,
                                    agent_id: "N/A".to_string(),
                                    content: IoOutput {
                                        streaming: false,
                                        content: "Delete run is not implemented yet.".to_string(),
                                        final_content: None,
                                    },
                                }
                            );
                        }
                        CommandEvent::ListRuns => match kernel.app.workspace.list_runs() {
                            Ok(runs) => {
                                let runs_str = if runs.is_empty() {
                                    "No runs.".to_string()
                                } else {
                                    runs.into_iter()
                                        .map(|run| {
                                            let lock = if run.run_lock.is_some() {
                                                "locked"
                                            } else {
                                                "available"
                                            };
                                            format!(
                                                "{} [{}] {}",
                                                run.run_metadata.run_id,
                                                lock,
                                                run.run_metadata.title
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                };
                                send_kernel_event_or_break!(
                                    kernel_tx,
                                    KernelToShellEvent::Output {
                                        run_id: "N/A".to_string(),
                                        agent_id: "N/A".to_string(),
                                        content: IoOutput {
                                            streaming: false,
                                            content: format!("Runs:\n{runs_str}"),
                                            final_content: None,
                                        },
                                    }
                                );
                            }
                            Err(e) => {
                                send_kernel_event_or_break!(
                                    kernel_tx,
                                    KernelToShellEvent::Output {
                                        run_id: "N/A".to_string(),
                                        agent_id: "N/A".to_string(),
                                        content: IoOutput {
                                            streaming: false,
                                            content: format!("Failed to list runs: {e}"),
                                            final_content: None,
                                        },
                                    }
                                );
                            }
                        },
                    },
                    ShellToKernelEvent::UserInput {
                        run_id,
                        agent_id,
                        content,
                    } => {
                        let output = IoOutput {
                            streaming: false,
                            content: format!("kernel received: {content}"),
                            final_content: None,
                        };
                        if kernel_tx
                            .send(KernelToShellEvent::Output {
                                run_id: run_id.clone(),
                                agent_id: agent_id.clone(),
                                content: output,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                        if kernel_tx
                            .send(KernelToShellEvent::Done { run_id, agent_id })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ShellToKernelEvent::LLMInput {
                        run_id,
                        agent_id,
                        content,
                    } => {
                        let output = IoOutput {
                            streaming: false,
                            content: format!("llm input queued: {content}"),
                            final_content: None,
                        };
                        if kernel_tx
                            .send(KernelToShellEvent::Output {
                                run_id,
                                agent_id,
                                content: output,
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ShellToKernelEvent::Cancel { run_id, agent_id } => {
                        if kernel_tx
                            .send(KernelToShellEvent::Done { run_id, agent_id })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ShellToKernelEvent::Shutdown => {
                        if let Err(e) = kernel.release_current_run_lock() {
                            send_kernel_event_or_break!(
                                kernel_tx,
                                KernelToShellEvent::Output {
                                    run_id: "N/A".to_string(),
                                    agent_id: "N/A".to_string(),
                                    content: IoOutput {
                                        streaming: false,
                                        content: format!("Failed to release run lock: {e}"),
                                        final_content: None,
                                    },
                                }
                            );
                        }
                        break;
                    }
                }
            }
        });

        (shell_tx, kernel_rx)
    }
}
