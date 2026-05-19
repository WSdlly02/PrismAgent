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
        AgentSnapshot, KernelSnapshot, KernelToShellEvent, ShellToKernelEvent, UserInput,
        UserKernelCommand, UserKernelCommandRequest,
    },
    model::kernel::Kernel,
    model::unit::{Unit, UnitRole},
};

const DEFAULT_RUN_UUID: &str = "none";
const DEFAULT_AGENT_UUID: &str = "none";

#[tokio::main]
async fn main() -> Result<()> {
    let kernel = Kernel::new()?;
    let (shell_tx, kernel_rx) = kernel.run();
    let mut tui_app = TuiApp::new(DEFAULT_RUN_UUID, DEFAULT_AGENT_UUID, shell_tx, kernel_rx);
    tui_app.push_system(
        "PrismAgent TUI started. Use /list, /new <title>, or /resume <run-uuid>. Esc/Ctrl-C cancels the current request. Ctrl-D exits.",
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
    snapshot: Option<KernelSnapshot>,
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
            snapshot: None,
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
    const PREFIX_WIDTH: usize = 8;

    fn new(source: &'static str, color: Color, content: impl Into<String>) -> Self {
        Self {
            source,
            color,
            content: content.into(),
        }
    }

    fn items(&self, content_width: usize) -> Vec<ListItem<'static>> {
        wrap_text(&self.content, content_width)
            .into_iter()
            .enumerate()
            .map(|(index, content)| {
                let prefix = if index == 0 {
                    format!("{:<7}", self.source)
                } else {
                    "       ".to_string()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default().fg(self.color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::raw(content),
                ]))
            })
            .collect()
    }
}

fn wrap_text(content: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw_line in content.replace('\t', "    ").split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let chars = raw_line.chars().collect::<Vec<_>>();
        for chunk in chars.chunks(width) {
            lines.push(chunk.iter().collect());
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
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

fn cancel_event(app: &TuiApp) -> ShellToKernelEvent {
    ShellToKernelEvent::KernelCommand(UserKernelCommandRequest {
        request_uuid: request_uuid(),
        command: UserKernelCommand::Cancel {
            run_uuid: if app.run_uuid == DEFAULT_RUN_UUID {
                None
            } else {
                Some(app.run_uuid.clone())
            },
            agent_uuid: if app.agent_uuid == DEFAULT_AGENT_UUID {
                None
            } else {
                Some(app.agent_uuid.clone())
            },
        },
    })
}

fn approve_event(app: &TuiApp, args: String) -> ShellToKernelEvent {
    ShellToKernelEvent::KernelCommand(UserKernelCommandRequest {
        request_uuid: request_uuid(),
        command: UserKernelCommand::Approve {
            run_uuid: if app.run_uuid == DEFAULT_RUN_UUID {
                None
            } else {
                Some(app.run_uuid.clone())
            },
            agent_uuid: if app.agent_uuid == DEFAULT_AGENT_UUID {
                None
            } else {
                Some(app.agent_uuid.clone())
            },
            args,
        },
    })
}

async fn request_cancel(app: &mut TuiApp) {
    if app.input_enabled {
        app.push_system("no active request to cancel");
        app.status = "idle".to_owned();
        return;
    }

    if app.shell_tx.send(cancel_event(app)).await.is_err() {
        app.push_error("kernel channel closed");
        app.status = "kernel disconnected".to_owned();
        return;
    }

    app.status = "cancel requested".to_owned();
}

async fn request_approve(app: &mut TuiApp, args: String) {
    if app.shell_tx.send(approve_event(app, args)).await.is_err() {
        app.push_error("kernel channel closed");
        app.status = "kernel disconnected".to_owned();
        return;
    }

    app.status = "approval sent".to_owned();
}

async fn handle_key(app: &mut TuiApp, key: KeyEvent) -> Result<()> {
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            request_cancel(app).await;
        }
        KeyCode::Esc => {
            request_cancel(app).await;
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            request_approve(app, "all".to_string()).await;
        }
        KeyCode::Enter => {
            let content = app.input.trim().to_owned();
            app.input.clear();
            if content.is_empty() {
                return Ok(());
            }
            app.status = "request sent".to_owned();
            match content.chars().nth(0) {
                // 以 '/' 开头的输入被视为UserKernelCommandRequest
                Some('/') => {
                    app.push_user(content.clone());
                    let command = content[1..].trim();
                    let event = match command {
                        "list" => command_event(UserKernelCommand::ListRuns),
                        "cancel" => command_event(UserKernelCommand::Cancel {
                            run_uuid: if app.run_uuid == DEFAULT_RUN_UUID {
                                None
                            } else {
                                Some(app.run_uuid.clone())
                            },
                            agent_uuid: if app.agent_uuid == DEFAULT_AGENT_UUID {
                                None
                            } else {
                                Some(app.agent_uuid.clone())
                            },
                        }),
                        "approve" => approve_event(app, "all".to_string()),
                        cmd if cmd.starts_with("approve ") => {
                            let args = cmd[8..].trim();
                            approve_event(
                                app,
                                if args.is_empty() {
                                    "all".to_string()
                                } else {
                                    args.to_string()
                                },
                            )
                        }
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
                            run_uuid: app.run_uuid.clone(),
                            agent_uuid: app.agent_uuid.clone(),
                            content,
                        }))
                        .await
                        .is_err()
                    {
                        app.push_error("kernel channel closed");
                        app.status = "kernel disconnected".to_owned();
                    } else {
                        app.input_enabled = false;
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
        match event {
            KernelToShellEvent::Snapshot { snapshot, .. } => {
                apply_snapshot(app, snapshot);
            }
            KernelToShellEvent::Patch { text, .. } => {
                app.push_system(text.clone());
                app.status = text;
                app.input_enabled = true;
            }
        }
    }
}

fn apply_snapshot(app: &mut TuiApp, snapshot: KernelSnapshot) {
    let run_uuid = snapshot.run_metadata.uuid.clone();
    app.run_uuid = run_uuid.clone();
    if app.agent_uuid == DEFAULT_AGENT_UUID
        || snapshot
            .agents
            .iter()
            .all(|agent| agent.agent.uuid != app.agent_uuid)
    {
        app.agent_uuid = snapshot.run_metadata.root_agent_uuid.clone();
    }

    let old_count = app
        .snapshot
        .as_ref()
        .and_then(|old_snapshot| find_agent(old_snapshot, &app.agent_uuid))
        .map(|agent| agent.units.len())
        .unwrap_or(0);
    if let Some(agent) = find_agent(&snapshot, &app.agent_uuid) {
        let new_units = agent.units.iter().skip(old_count).collect::<Vec<_>>();
        push_units(app, &new_units);
        app.status = format!(
            "active: {} agent={} units={}",
            run_uuid,
            app.agent_uuid,
            agent.units.len()
        );
    } else {
        app.status = format!("active: {run_uuid}");
    }
    app.input_enabled = true;
    app.snapshot = Some(snapshot);
}

fn find_agent<'a>(snapshot: &'a KernelSnapshot, agent_uuid: &str) -> Option<&'a AgentSnapshot> {
    snapshot
        .agents
        .iter()
        .find(|agent| agent.agent.uuid == agent_uuid)
}

fn push_units(app: &mut TuiApp, units: &[&Unit]) {
    for unit in units {
        let content = unit
            .metadata
            .get("preview")
            .or_else(|| unit.metadata.get("content"))
            .cloned()
            .unwrap_or_default();
        if content.is_empty() {
            continue;
        }
        match unit.role {
            UnitRole::User => app.push_user(content),
            UnitRole::Assistant => app.push_kernel(content),
            UnitRole::System => app.push_system(content),
            UnitRole::Tool => app.push_kernel(content),
        }
    }
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

    let transcript_height = chunks[1].height.saturating_sub(2) as usize;
    let transcript_inner_width = chunks[1].width.saturating_sub(2) as usize;
    let content_width = transcript_inner_width
        .saturating_sub(LogLine::PREFIX_WIDTH)
        .max(1);
    let all_items = app
        .lines
        .iter()
        .flat_map(|line| line.items(content_width))
        .collect::<Vec<_>>();
    let skip_count = all_items.len().saturating_sub(transcript_height);
    let items = all_items.into_iter().skip(skip_count).collect::<Vec<_>>();
    let transcript =
        List::new(items).block(Block::default().borders(Borders::ALL).title("Transcript"));
    frame.render_widget(transcript, chunks[1]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title("Input"))
        .wrap(Wrap { trim: false });
    frame.render_widget(input, chunks[2]);

    let help = Paragraph::new("Enter send  Ctrl-Y approve all  Esc/Ctrl-C cancel  Ctrl-D quit");
    frame.render_widget(help, chunks[3]);
}
