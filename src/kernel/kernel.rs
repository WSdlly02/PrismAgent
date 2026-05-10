use crate::model::app::App;
use crate::model::asyncioinstance::IoOutput;
use crate::model::event::{CommandEvent, KernelEvent, ShellEvent};
use crate::model::kernel::{Kernel, KernelRuntime};
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
        let current_run = self.app.workspace.resume_run(run_id)?;
        self.runtime = Some(KernelRuntime {
            current_run,
            handles: HashMap::new(),
        });
        Ok(())
    }
    pub fn initialize_runtime_with_new_run(&mut self, title: &str) -> Result<()> {
        let current_run = self.app.workspace.create_run_and_resume(title)?;
        self.runtime = Some(KernelRuntime {
            current_run,
            handles: HashMap::new(),
        });
        Ok(())
    }
    /// 启动内核事件循环，返回与 TUI 通信的两端
    pub fn run(self) -> (mpsc::Sender<ShellEvent>, mpsc::Receiver<KernelEvent>) {
        let (shell_tx, mut shell_rx) = mpsc::channel::<ShellEvent>(64);
        let (kernel_tx, kernel_rx) = mpsc::channel::<KernelEvent>(64);

        tokio::spawn(async move {
            let _kernel = self;
            while let Some(event) = shell_rx.recv().await {
                match event {
                    ShellEvent::CommandInput { command } => {
                        match command {
                            CommandEvent::NewRun { title } => {
                                // Handle new run command
                            }
                            CommandEvent::ResumeRun { run_id } => {
                                // Handle resume run command
                            }
                            CommandEvent::DeleteRun { run_id } => {
                                // Handle delete run command
                            }
                            CommandEvent::ListRuns => {
                                // Handle list runs command
                            }
                        }
                    }
                    ShellEvent::UserInput {
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
                            .send(KernelEvent::Output {
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
                            .send(KernelEvent::Done { run_id, agent_id })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ShellEvent::LLMInput {
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
                            .send(KernelEvent::Output {
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
                    ShellEvent::Cancel { run_id, agent_id } => {
                        if kernel_tx
                            .send(KernelEvent::Done { run_id, agent_id })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    ShellEvent::Shutdown => break,
                }
            }
        });

        (shell_tx, kernel_rx)
    }
}
