use anyhow::Result;
use std::{io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use tokio::sync::mpsc;
use uuid::Uuid;

use prismagent::{
    model::event::{
        InputTarget, KernelToShellEvent, KernelToShellPayload, KernelView, RuntimeStatus,
        ShellToKernelEvent, StatusLevel, UserInput, UserKernelCommand, UserKernelCommandRequest,
        UserShellCommandRequest,
    },
    model::kernel::Kernel,
};

const DEFAULT_RUN_UUID: &str = "none";
const DEFAULT_AGENT_UUID: &str = "none";

#[tokio::main]
async fn main() -> Result<()> {
    let kernel = Kernel::new()?;
    let (shell_tx, kernel_rx) = kernel.run();
    let mut tui_app = TuiApp::new(DEFAULT_RUN_UUID, DEFAULT_AGENT_UUID, shell_tx, kernel_rx);
    tui_app.push_system(
        "PrismAgent TUI started. Use /list, /new <title>, or /resume <run-uuid>. Esc or Ctrl-C exits.",
    );

    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;

    let result = run_loop(&mut terminal, &mut tui_app).await;
    let _ = tui_app
        .shell_tx
        .send(command_event(UserKernelCommand::Shutdown))
        .await;
    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
) -> Result<()> {
    loop {
        drain_kernel_events(app);
        terminal.draw(|frame| draw(frame, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            handle_key(app, key).await?;
        }

        tokio::task::yield_now().await;
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

struct TuiApp {
    run_uuid: String,
    agent_uuid: String,
    input: String,
    input_enabled: bool,
    status: String,
    lines: Vec<LogLine>,
    should_quit: bool,
    shell_tx: mpsc::Sender<ShellToKernelEvent>,
    kernel_rx: mpsc::Receiver<KernelToShellEvent>,
}

impl TuiApp {
    fn new(
        run_uuid: &str,
        agent_uuid: &str,
        shell_tx: mpsc::Sender<ShellToKernelEvent>,
        kernel_rx: mpsc::Receiver<KernelToShellEvent>,
    ) -> Self {
        Self {
            run_uuid: run_uuid.to_owned(),
            agent_uuid: agent_uuid.to_owned(),
            input: String::new(),
            input_enabled: true,
            status: "idle".to_owned(),
            lines: Vec::new(),
            should_quit: false,
            shell_tx,
            kernel_rx,
        }
    }

    fn push_user(&mut self, content: impl Into<String>) {
        self.lines.push(LogLine::new("user", Color::Cyan, content));
        self.truncate_lines();
    }

    fn push_kernel(&mut self, content: impl Into<String>) {
        self.lines
            .push(LogLine::new("kernel", Color::Green, content));
        self.truncate_lines();
    }

    fn push_error(&mut self, content: impl Into<String>) {
        self.lines.push(LogLine::new("error", Color::Red, content));
        self.truncate_lines();
    }

    fn push_system(&mut self, content: impl Into<String>) {
        self.lines
            .push(LogLine::new("system", Color::Yellow, content));
        self.truncate_lines();
    }

    fn truncate_lines(&mut self) {
        const MAX_LINES: usize = 500;
        if self.lines.len() > MAX_LINES {
            let overflow = self.lines.len() - MAX_LINES;
            self.lines.drain(0..overflow);
        }
    }
}

struct LogLine {
    source: &'static str,
    color: Color,
    content: String,
}

impl LogLine {
    fn new(source: &'static str, color: Color, content: impl Into<String>) -> Self {
        Self {
            source,
            color,
            content: content.into(),
        }
    }

    fn item(&self) -> ListItem<'_> {
        ListItem::new(Line::from(vec![
            Span::styled(
                format!("{:<7}", self.source),
                Style::default().fg(self.color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::raw(self.content.as_str()),
        ]))
    }
}

fn request_uuid() -> String {
    Uuid::now_v7().to_string()
}

fn command_event(command: UserKernelCommand) -> ShellToKernelEvent {
    ShellToKernelEvent::KernelCommand(UserKernelCommandRequest {
        request_uuid: request_uuid(),
        command,
    })
}

async fn handle_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Enter => {
            let content = app.input.trim().to_owned();
            app.input.clear();
            if content.is_empty() {
                return Ok(());
            }
            app.push_user(content.clone());
            app.status = "request sent".to_owned();
            match content.chars().nth(0) {
                // 以 '/' 开头的输入被视为UserKernelCommandRequest
                Some('/') => {
                    let command = content[1..].trim();
                    let event = match command {
                        "list" => command_event(UserKernelCommand::ListRuns),
                        "context" => command_event(UserKernelCommand::FetchCurrentContext),
                        "cancel" => command_event(UserKernelCommand::Cancel {
                            run_uuid: None,
                            agent_uuid: None,
                            asyncioinstance_uuid: None,
                        }),
                        "new" => command_event(UserKernelCommand::NewRun { title: None }),
                        cmd if cmd.starts_with("new ") => {
                            let title = cmd[4..].trim();
                            command_event(UserKernelCommand::NewRun {
                                title: if title.is_empty() {
                                    None
                                } else {
                                    Some(title.to_owned())
                                },
                            })
                        }
                        cmd if cmd.starts_with("resume ") => {
                            let run_uuid = cmd[7..].trim().to_owned();
                            command_event(UserKernelCommand::ResumeRun { run_uuid })
                        }
                        cmd if cmd.starts_with("delete ") => {
                            let run_uuid = cmd[7..].trim().to_owned();
                            command_event(UserKernelCommand::DeleteRun { run_uuid })
                        }
                        cmd if cmd.starts_with("snapshot") => {
                            let snapshot_uid = cmd
                                .strip_prefix("snapshot")
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_owned);
                            command_event(UserKernelCommand::Snapshot {
                                run_uuid: None,
                                snapshot_uid,
                            })
                        }
                        _ => {
                            app.push_error(format!("unknown command: {command}"));
                            return Ok(());
                        }
                    };
                    if app.shell_tx.send(event).await.is_err() {
                        app.push_error("kernel channel closed");
                        app.status = "kernel disconnected".to_owned();
                    }
                }
                // 以 '!' 开头的输入被视为直接的 shell 命令
                Some('!') => {
                    let command = content[1..].trim();
                    let event = ShellToKernelEvent::ShellCommand(UserShellCommandRequest {
                        command: command.to_owned(),
                    });
                    if app.shell_tx.send(event).await.is_err() {
                        app.push_error("kernel channel closed");
                        app.status = "kernel disconnected".to_owned();
                    }
                }
                // 其他输入被视为针对当前 agent 的用户输入
                _ => {
                    if !app.input_enabled {
                        app.push_error("input is disabled while the selected target is running");
                        return Ok(());
                    }
                    if app.run_uuid == DEFAULT_RUN_UUID || app.agent_uuid == DEFAULT_AGENT_UUID {
                        app.push_error("no active run; use /new <title> or /resume <run-uuid>");
                        app.status = "no active run".to_owned();
                        return Ok(());
                    }
                    if app
                        .shell_tx
                        .send(ShellToKernelEvent::Input(UserInput {
                            request_uuid: request_uuid(),
                            target: InputTarget::Agent {
                                agent_uuid: app.agent_uuid.clone(),
                            },
                            content,
                        }))
                        .await
                        .is_err()
                    {
                        app.push_error("kernel channel closed");
                        app.status = "kernel disconnected".to_owned();
                    }
                }
            }
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(ch) => {
            app.input.push(ch);
        }
        _ => {}
    }
    Ok(())
}

fn drain_kernel_events(app: &mut TuiApp) {
    while let Ok(event) = app.kernel_rx.try_recv() {
        match event.payload {
            KernelToShellPayload::Stdout(stream) => {
                let run_uuid = stream.run_uuid.unwrap_or_else(|| "N/A".to_string());
                let agent_uuid = stream.agent_uuid.unwrap_or_else(|| "N/A".to_string());
                app.status = format!("output from {run_uuid}/{agent_uuid}");
                app.run_uuid = if run_uuid == "N/A" {
                    app.run_uuid.clone()
                } else {
                    run_uuid
                };
                app.agent_uuid = if agent_uuid == "N/A" {
                    app.agent_uuid.clone()
                } else {
                    agent_uuid
                };
                app.push_kernel(render_units(&stream.units));
            }
            KernelToShellPayload::Stderr(stream) => {
                let run_uuid = stream.run_uuid.unwrap_or_else(|| "N/A".to_string());
                let agent_uuid = stream.agent_uuid.unwrap_or_else(|| "N/A".to_string());
                app.status = format!("error from {run_uuid}/{agent_uuid}");
                app.push_error(render_units(&stream.units));
            }
            KernelToShellPayload::Status(status) => {
                let runtime_status = status.runtime_status;
                if let Some(run_uuid) = status.run_uuid {
                    app.run_uuid = run_uuid;
                }
                if let Some(agent_uuid) = status.agent_uuid {
                    app.agent_uuid = agent_uuid;
                }
                match runtime_status {
                    Some(RuntimeStatus::Accepted) | Some(RuntimeStatus::Running) => {
                        app.input_enabled = false;
                    }
                    Some(RuntimeStatus::WaitingInput)
                    | Some(RuntimeStatus::Done)
                    | Some(RuntimeStatus::Failed)
                    | Some(RuntimeStatus::Cancelled) => {
                        app.input_enabled = true;
                    }
                    None => {}
                }
                app.status = status.message.clone();
                match status.level {
                    StatusLevel::Info => app.push_system(status.message),
                    StatusLevel::Warn => app.push_system(format!("warn: {}", status.message)),
                    StatusLevel::Error => app.push_error(status.message),
                }
            }
            KernelToShellPayload::View(view) => match view {
                KernelView::Runs { runs } => {
                    if runs.is_empty() {
                        app.push_system("No runs.");
                    } else {
                        let lines = runs
                            .into_iter()
                            .map(|run| {
                                let lock = if run.locked { "locked" } else { "available" };
                                format!("{} [{lock}] {} {:?}", run.uuid, run.title, run.status)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        app.push_system(format!("Runs:\n{lines}"));
                    }
                    app.status = "runs listed".to_owned();
                }
                KernelView::CurrentContext {
                    run_uuid,
                    agent_uuid,
                    title,
                    unit_count,
                    head_unit_uuid,
                } => {
                    app.run_uuid = run_uuid.unwrap_or_else(|| DEFAULT_RUN_UUID.to_string());
                    app.agent_uuid = agent_uuid.unwrap_or_else(|| DEFAULT_AGENT_UUID.to_string());
                    app.status = format!("active: {}", app.run_uuid);
                    app.push_system(format!(
                        "context run={} agent={} title={} units={} head={}",
                        app.run_uuid,
                        app.agent_uuid,
                        title.unwrap_or_else(|| "none".to_string()),
                        unit_count,
                        head_unit_uuid.unwrap_or_else(|| "none".to_string())
                    ));
                }
                KernelView::SnapshotCreated {
                    run_uuid,
                    snapshot_uid,
                    name,
                } => {
                    app.status = format!("snapshot created: {snapshot_uid}");
                    app.push_system(format!(
                        "snapshot created: run={run_uuid} uid={snapshot_uid} name={}",
                        name.unwrap_or_else(|| "none".to_string())
                    ));
                }
                KernelView::RunDeleted { run_uuid } => {
                    app.status = format!("run deleted: {run_uuid}");
                    app.push_system(format!("run deleted: {run_uuid}"));
                }
            },
        }
    }
}

fn render_units(units: &[prismagent::model::unit::Unit]) -> String {
    units
        .iter()
        .filter_map(|unit| {
            unit.metadata
                .get("content")
                .or_else(|| unit.metadata.get("preview"))
                .cloned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw(frame: &mut Frame<'_>, app: &TuiApp) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "PrismAgent",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  run={} agent={}  input={}  status={}",
            app.run_uuid,
            app.agent_uuid,
            if app.input_enabled { "on" } else { "off" },
            app.status
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).title("Session"));
    frame.render_widget(title, chunks[0]);

    let visible_items = app
        .lines
        .iter()
        .rev()
        .take(chunks[1].height.saturating_sub(2) as usize)
        .collect::<Vec<_>>();
    let items = visible_items
        .into_iter()
        .rev()
        .map(LogLine::item)
        .collect::<Vec<_>>();
    let transcript =
        List::new(items).block(Block::default().borders(Borders::ALL).title("Transcript"));
    frame.render_widget(transcript, chunks[1]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("Input"))
        .wrap(Wrap { trim: false });
    frame.render_widget(input, chunks[2]);

    let help = Paragraph::new("Enter send  Esc/Ctrl-C quit");
    frame.render_widget(help, chunks[3]);
}
