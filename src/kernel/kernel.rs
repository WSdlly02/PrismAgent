use crate::kernel::pipeline::{input_pipeline, output_pipeline};
use crate::kernel::processor::{spawn_llm_instance, spawn_tool_instance};
use crate::model::agent::Agent;
use crate::model::app::App;
use crate::model::asyncioinstance::{
    AsyncIoBox, AsyncIoInstanceExecutionMode, AsyncIoInstanceRole, InstanceSignal,
};
use crate::model::event::{
    AgentSnapshot, InstanceToKernelEvent, KernelSnapshot, KernelToShellEvent, ShellToKernelEvent,
    UserKernelCommand,
};
use crate::model::kernel::{AsyncIoHandleEntry, AsyncIoOwner, Kernel, KernelRuntime};
use crate::model::run::{Run, RunMetadata, RunSummary};
use anyhow::{Result, anyhow};
use genai::Client as GenAIClient;
use genai::resolver::{AuthData, AuthResolver, Error as ResolverError};
use std::collections::HashMap;
use tokio::sync::mpsc;

impl Kernel {
    /// 新建 Kernel，不具有运行时。
    pub fn new() -> Result<Self> {
        let app = App::new().map_err(|e| anyhow!("Failed to initialize App: {e}"))?;
        let api_key = app.model_config.final_api_key.clone();
        let auth_resolver =
            AuthResolver::from_resolver_fn(|_| -> Result<Option<AuthData>, ResolverError> {
                Ok(Some(AuthData::from_single(api_key)))
            });
        let llm_client = GenAIClient::builder()
            .with_auth_resolver(auth_resolver)
            .build();
        Ok(Self {
            app,
            llm_client,
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
                ($event:expr) => {
                    if kernel_tx.send($event).await.is_err() {
                        break;
                    }
                };
            }

            macro_rules! emit_patch {
                ($correlation_uuid:expr, $text:expr) => {
                    emit!(KernelToShellEvent::Patch {
                        correlation_uuid: $correlation_uuid,
                        text: $text,
                    });
                };
            }

            macro_rules! emit_snapshot {
                ($correlation_uuid:expr) => {
                    match build_current_snapshot(&kernel) {
                        Ok(snapshot) => emit!(KernelToShellEvent::Snapshot {
                            correlation_uuid: $correlation_uuid,
                            snapshot,
                        }),
                        Err(error) => {
                            emit_patch!(
                                $correlation_uuid,
                                format!("Failed to build kernel snapshot: {error}")
                            );
                        }
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
                                        emit_snapshot!(correlation_uuid);
                                    }
                                    Err(error) => {
                                        emit_patch!(
                                            correlation_uuid,
                                            format!("Failed to create and resume new run: {error}")
                                        );
                                    }
                                }
                            }
                            UserKernelCommand::ResumeRun { run_uuid } => {
                                match kernel.initialize_runtime(&run_uuid) {
                                    Ok(()) => {
                                        emit_snapshot!(correlation_uuid);
                                    }
                                    Err(error) => {
                                        emit_patch!(
                                            correlation_uuid,
                                            format!("Failed to resume run: {error}")
                                        );
                                    }
                                }
                            }
                            UserKernelCommand::DeleteRun { run_uuid } => {
                                emit_patch!(
                                    correlation_uuid,
                                    format!("Delete run is not implemented yet: {run_uuid}")
                                );
                            }
                            UserKernelCommand::ListRuns => match kernel.app.workspace.list_runs() {
                                Ok(runs) => {
                                    emit_patch!(correlation_uuid, format_run_list(&runs));
                                }
                                Err(error) => {
                                    emit_patch!(
                                        correlation_uuid,
                                        format!("Failed to list runs: {error}")
                                    );
                                }
                            },
                            UserKernelCommand::Cancel {
                                run_uuid,
                                agent_uuid,
                            } => {
                                let Some(runtime) = kernel.runtime.as_mut() else {
                                    emit_patch!(
                                        correlation_uuid,
                                        "No active run to cancel.".to_string()
                                    );
                                    continue;
                                };
                                let target_run_uuid = run_uuid
                                    .unwrap_or_else(|| runtime.current_run.run_metadata.uuid.clone());
                                if target_run_uuid != runtime.current_run.run_metadata.uuid {
                                    emit_patch!(
                                        correlation_uuid,
                                        "Cannot cancel a task outside the active run.".to_string()
                                    );
                                    continue;
                                }
                                let target_agent_uuid = agent_uuid.unwrap_or_else(|| {
                                    runtime.current_run.run_metadata.root_agent_uuid.clone()
                                });
                                let Some((instance_uuid, entry)) =
                                    runtime.handles.iter().find(|(_, entry)| {
                                        entry.run_uuid == target_run_uuid
                                            && entry.execution_mode
                                                == crate::model::asyncioinstance::AsyncIoInstanceExecutionMode::Blocking
                                            && matches!(
                                                &entry.owner,
                                                AsyncIoOwner::Agent { agent_uuid }
                                                    if agent_uuid == &target_agent_uuid
                                            )
                                    })
                                else {
                                    emit_patch!(
                                        correlation_uuid,
                                        "No active request to cancel.".to_string()
                                    );
                                    continue;
                                };
                                let instance_uuid = instance_uuid.clone();
                                let signal_result =
                                    entry.handle.signal_tx.send(InstanceSignal::Terminate).await;
                                if let Err(error) = signal_result {
                                    runtime.handles.remove(&instance_uuid);
                                    emit_patch!(
                                        correlation_uuid,
                                        format!("Failed to cancel active request: {error}")
                                    );
                                    continue;
                                }
                                emit_patch!(
                                    correlation_uuid,
                                    "Cancel signal sent.".to_string()
                                );
                            }
                            UserKernelCommand::Approve {
                                run_uuid,
                                agent_uuid,
                                args,
                            } => {
                                let Some(runtime) = kernel.runtime.as_mut() else {
                                    emit_patch!(
                                        correlation_uuid,
                                        "No active run to approve.".to_string()
                                    );
                                    continue;
                                };
                                let target_run_uuid = run_uuid
                                    .unwrap_or_else(|| runtime.current_run.run_metadata.uuid.clone());
                                if target_run_uuid != runtime.current_run.run_metadata.uuid {
                                    emit_patch!(
                                        correlation_uuid,
                                        "Cannot approve a task outside the active run.".to_string()
                                    );
                                    continue;
                                }
                                let target_agent_uuid = agent_uuid.unwrap_or_else(|| {
                                    runtime.current_run.run_metadata.root_agent_uuid.clone()
                                });
                                let Some((instance_uuid, entry)) =
                                    runtime.handles.iter().find(|(_, entry)| {
                                        entry.run_uuid == target_run_uuid
                                            && entry.role == AsyncIoInstanceRole::Tool
                                            && entry.execution_mode
                                                == AsyncIoInstanceExecutionMode::Blocking
                                            && matches!(
                                                &entry.owner,
                                                AsyncIoOwner::Agent { agent_uuid }
                                                    if agent_uuid == &target_agent_uuid
                                            )
                                    })
                                else {
                                    emit_patch!(
                                        correlation_uuid,
                                        "No pending tool request to approve.".to_string()
                                    );
                                    continue;
                                };
                                let instance_uuid = instance_uuid.clone();
                                let signal_result = entry
                                    .handle
                                    .signal_tx
                                    .send(InstanceSignal::Approve { args })
                                    .await;
                                if let Err(error) = signal_result {
                                    runtime.handles.remove(&instance_uuid);
                                    emit_patch!(
                                        correlation_uuid,
                                        format!("Failed to approve pending tool request: {error}")
                                    );
                                    continue;
                                }
                                emit_patch!(
                                    correlation_uuid,
                                    "Approve signal sent.".to_string()
                                );
                            }
                            UserKernelCommand::Snapshot { .. } => {
                                emit_patch!(
                                    correlation_uuid,
                                    "Snapshot is not implemented yet.".to_string()
                                );
                            }
                            UserKernelCommand::Shutdown => {
                                match kernel.release_current_run_lock() {
                                    Ok(()) => {
                                        emit_patch!(
                                            correlation_uuid,
                                            "Kernel shutdown.".to_string()
                                        );
                                    }
                                    Err(error) => {
                                        emit_patch!(
                                            correlation_uuid,
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
                            emit_patch!(
                                Some(input.request_uuid),
                                "No active run; use /new <title> or /resume <run-uuid> first."
                                    .to_string()
                            );
                            continue;
                        };

                        let run_uuid = input.run_uuid;
                        let agent_uuid = input.agent_uuid;
                        if run_uuid != runtime.current_run.run_metadata.uuid {
                            emit_patch!(
                                Some(input.request_uuid),
                                "Input run does not match active run.".to_string()
                            );
                            continue;
                        }
                        let agent_path = runtime
                            .current_run
                            .root
                            .join("agents")
                            .join(format!("{agent_uuid}.json"));
                        if !agent_path.is_file() {
                            emit_patch!(
                                Some(input.request_uuid),
                                "Target agent does not exist in active run.".to_string()
                            );
                            continue;
                        }
                        if runtime.handles.iter().any(|(_, entry)| {
                            entry.run_uuid == run_uuid
                                && entry.execution_mode == AsyncIoInstanceExecutionMode::Blocking
                                && matches!(
                                    &entry.owner,
                                    AsyncIoOwner::Agent { agent_uuid: owner_agent_uuid }
                                        if owner_agent_uuid == &agent_uuid
                                )
                        }) {
                            emit_patch!(
                                Some(input.request_uuid),
                                "Target agent already has an active blocking request.".to_string()
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
                                emit_patch!(
                                    Some(input.request_uuid),
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
                                emit_patch!(
                                    Some(input.request_uuid),
                                    format!("Failed to create AsyncIoInstance: {error}")
                                );
                                continue;
                            }
                        };
                        let (instance, handle) = io_box.split();
                        let instance_uuid = instance.uuid.clone();
                        if let Err(error) = handle.stdin.try_send(input_units) {
                            emit_patch!(
                                Some(input.request_uuid),
                                format!("Failed to send input to AsyncIoInstance: {error}")
                            );
                            continue;
                        }
                        runtime.handles.insert(
                            instance_uuid,
                            AsyncIoHandleEntry {
                                run_uuid: run_uuid.clone(),
                                owner: AsyncIoOwner::Agent {
                                    agent_uuid: agent_uuid.clone(),
                                },
                                role: instance.role,
                                execution_mode: instance.execution_mode,
                                handle,
                            },
                        );

                        spawn_llm_instance(
                            kernel.llm_client.clone(),
                            kernel.app.model_config.final_model.clone(),
                            runtime.current_run.root.clone(),
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
                            emit_patch!(
                                instance_event.correlation_uuid,
                                "Instance returned output without active run.".to_string()
                            );
                            continue;
                        };

                        if runtime.current_run.run_metadata.uuid != instance_event.run_uuid {
                            runtime.handles.remove(&instance_event.asyncioinstance_uuid);
                            emit_patch!(
                                instance_event.correlation_uuid,
                                "Instance output does not belong to current run.".to_string()
                            );
                            continue;
                        }

                        let instance_role = runtime
                            .handles
                            .get(&instance_event.asyncioinstance_uuid)
                            .map(|entry| entry.role)
                            .unwrap_or(AsyncIoInstanceRole::Unknown);

                        let committed_units = match output_pipeline(
                            &kernel.app.workspace,
                            &mut runtime.current_run,
                            &instance_event.agent_uuid,
                            instance_event.units,
                        ) {
                            Ok(units) => units,
                            Err(error) => {
                                runtime.handles.remove(&instance_event.asyncioinstance_uuid);
                                emit_patch!(
                                    instance_event.correlation_uuid,
                                    format!("Output pipeline failed: {error}")
                                );
                                continue;
                            }
                        };
                        runtime.handles.remove(&instance_event.asyncioinstance_uuid);

                        if instance_event.is_tool_calls {
                            let io_box = match AsyncIoBox::new(instance_tx.clone())
                                .with_role(AsyncIoInstanceRole::Tool)
                                .done()
                            {
                                Ok(io_box) => io_box,
                                Err(error) => {
                                    emit_patch!(
                                        instance_event.correlation_uuid,
                                        format!("Failed to create tool AsyncIoInstance: {error}")
                                    );
                                    continue;
                                }
                            };
                            let (instance, handle) = io_box.split();
                            let instance_uuid = instance.uuid.clone();
                            if let Err(error) = handle.stdin.try_send(committed_units) {
                                emit_patch!(
                                    instance_event.correlation_uuid,
                                    format!("Failed to send tool input to AsyncIoInstance: {error}")
                                );
                                continue;
                            }
                            runtime.handles.insert(
                                instance_uuid,
                                AsyncIoHandleEntry {
                                    run_uuid: instance_event.run_uuid.clone(),
                                    owner: AsyncIoOwner::Agent {
                                        agent_uuid: instance_event.agent_uuid.clone(),
                                    },
                                    role: instance.role,
                                    execution_mode: instance.execution_mode,
                                    handle,
                                },
                            );
                            spawn_tool_instance(
                                runtime.current_run.root.clone(),
                                instance_event
                                    .correlation_uuid
                                    .clone()
                                    .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                                instance_event.run_uuid,
                                instance_event.agent_uuid,
                                instance,
                            );
                            match build_run_snapshot(&runtime.current_run) {
                                Ok(snapshot) => emit!(KernelToShellEvent::Snapshot {
                                    correlation_uuid: instance_event.correlation_uuid.clone(),
                                    snapshot,
                                }),
                                Err(error) => {
                                    emit_patch!(
                                        instance_event.correlation_uuid.clone(),
                                        format!("Failed to build kernel snapshot: {error}")
                                    );
                                }
                            }
                            emit_patch!(
                                instance_event.correlation_uuid.clone(),
                                "Tool calls are pending approval. Use /approve all, /approve 1 2, or Ctrl-Y."
                                    .to_string()
                            );
                        } else if instance_role == AsyncIoInstanceRole::Tool {
                            let io_box = match AsyncIoBox::new(instance_tx.clone())
                                .with_role(AsyncIoInstanceRole::LLM)
                                .done()
                            {
                                Ok(io_box) => io_box,
                                Err(error) => {
                                    emit_patch!(
                                        instance_event.correlation_uuid,
                                        format!("Failed to create LLM AsyncIoInstance: {error}")
                                    );
                                    continue;
                                }
                            };
                            let (instance, handle) = io_box.split();
                            let instance_uuid = instance.uuid.clone();
                            if let Err(error) = handle.stdin.try_send(committed_units) {
                                emit_patch!(
                                    instance_event.correlation_uuid,
                                    format!("Failed to send LLM input to AsyncIoInstance: {error}")
                                );
                                continue;
                            }
                            runtime.handles.insert(
                                instance_uuid,
                                AsyncIoHandleEntry {
                                    run_uuid: instance_event.run_uuid.clone(),
                                    owner: AsyncIoOwner::Agent {
                                        agent_uuid: instance_event.agent_uuid.clone(),
                                    },
                                    role: instance.role,
                                    execution_mode: instance.execution_mode,
                                    handle,
                                },
                            );
                            spawn_llm_instance(
                                kernel.llm_client.clone(),
                                kernel.app.model_config.final_model.clone(),
                                runtime.current_run.root.clone(),
                                instance_event
                                    .correlation_uuid
                                    .clone()
                                    .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
                                instance_event.run_uuid,
                                instance_event.agent_uuid,
                                instance,
                            );
                            match build_run_snapshot(&runtime.current_run) {
                                Ok(snapshot) => emit!(KernelToShellEvent::Snapshot {
                                    correlation_uuid: instance_event.correlation_uuid.clone(),
                                    snapshot,
                                }),
                                Err(error) => {
                                    emit_patch!(
                                        instance_event.correlation_uuid.clone(),
                                        format!("Failed to build kernel snapshot: {error}")
                                    );
                                }
                            }
                        } else {
                            match build_run_snapshot(&runtime.current_run) {
                                Ok(snapshot) => emit!(KernelToShellEvent::Snapshot {
                                    correlation_uuid: instance_event.correlation_uuid.clone(),
                                    snapshot,
                                }),
                                Err(error) => {
                                    emit_patch!(
                                        instance_event.correlation_uuid.clone(),
                                        format!("Failed to build kernel snapshot: {error}")
                                    );
                                }
                            }
                        }
                    }
                }
            }
        });

        (shell_tx, kernel_rx)
    }
}

fn build_current_snapshot(kernel: &Kernel) -> Result<KernelSnapshot> {
    let runtime = kernel
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow!("No active run"))?;
    build_run_snapshot(&runtime.current_run)
}

fn build_run_snapshot(run: &Run) -> Result<KernelSnapshot> {
    let metadata_path = run.root.join("metadata.json");
    let run_metadata: RunMetadata = serde_json::from_slice(
        &std::fs::read(&metadata_path)
            .map_err(|e| anyhow!("Failed to read run metadata {:?}: {}", metadata_path, e))?,
    )
    .map_err(|e| anyhow!("Failed to parse run metadata {:?}: {}", metadata_path, e))?;

    let agents_dir = run.root.join("agents");
    let mut agents = Vec::new();
    for entry in std::fs::read_dir(&agents_dir)
        .map_err(|e| anyhow!("Failed to read agents dir {:?}: {}", agents_dir, e))?
    {
        let path = entry?.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let agent: Agent = serde_json::from_slice(
            &std::fs::read(&path)
                .map_err(|e| anyhow!("Failed to read agent store {:?}: {}", path, e))?,
        )
        .map_err(|e| anyhow!("Failed to parse agent store {:?}: {}", path, e))?;
        let mut units = Vec::with_capacity(agent.unit_chain.len());
        for unit_uuid in &agent.unit_chain {
            let unit_path = run.root.join("units").join(format!("{unit_uuid}.json"));
            units.push(Run::read_unit_store(&unit_path)?);
        }
        agents.push(AgentSnapshot { agent, units });
    }
    agents.sort_by(|left, right| left.agent.uuid.cmp(&right.agent.uuid));

    Ok(KernelSnapshot {
        run_metadata,
        agents,
    })
}

fn format_run_list(runs: &[RunSummary]) -> String {
    if runs.is_empty() {
        return "No runs.".to_string();
    }
    let lines = runs
        .iter()
        .map(|run| {
            let lock = if run.locked { "locked" } else { "available" };
            format!("{} [{lock}] {} {:?}", run.uuid, run.title, run.status)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("Runs:\n{lines}")
}
