use anyhow::Result;
use std::{io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Position},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use tokio::sync::mpsc;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
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
const INLINE_HEIGHT: u16 = 7;

#[tokio::main]
async fn main() -> Result<()> {
    let kernel = Kernel::new()?;
    let (shell_tx, kernel_rx) = kernel.run();
    let mut app = TuiApp::new(DEFAULT_RUN_UUID, DEFAULT_AGENT_UUID, shell_tx, kernel_rx);

    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;
    insert_log(
        &mut terminal,
        "system",
        Color::Yellow,
        "PrismAgent started. Use /list, /new <title>, or /resume <run-uuid>.",
    )?;

    let result = run_loop(&mut terminal, &mut app).await;
    let _ = app
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
        drain_kernel_events(terminal, app)?;
        terminal.draw(|frame| draw(frame, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => handle_key(terminal, app, key).await?,
                Event::Resize(_, _) => {
                    terminal.autoresize()?;
                }
                _ => {}
            }
        }

        tokio::task::yield_now().await;
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    Ok(Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Inline(INLINE_HEIGHT),
        },
    )?)
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

struct TuiApp {
    run_uuid: String,
    agent_uuid: String,
    input: String,
    input_cursor: usize,
    input_enabled: bool,
    snapshot: Option<KernelSnapshot>,
    status: String,
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
            input_cursor: 0,
            input_enabled: true,
            snapshot: None,
            status: "idle".to_string(),
            should_quit: false,
            shell_tx,
            kernel_rx,
        }
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

fn active_run_uuid(app: &TuiApp) -> Option<String> {
    if app.run_uuid == DEFAULT_RUN_UUID {
        None
    } else {
        Some(app.run_uuid.clone())
    }
}

fn active_agent_uuid(app: &TuiApp) -> Option<String> {
    if app.agent_uuid == DEFAULT_AGENT_UUID {
        None
    } else {
        Some(app.agent_uuid.clone())
    }
}

fn cancel_event(app: &TuiApp) -> ShellToKernelEvent {
    command_event(UserKernelCommand::Cancel {
        run_uuid: active_run_uuid(app),
        agent_uuid: active_agent_uuid(app),
    })
}

fn approve_event(app: &TuiApp, args: String) -> ShellToKernelEvent {
    command_event(UserKernelCommand::Approve {
        run_uuid: active_run_uuid(app),
        agent_uuid: active_agent_uuid(app),
        args,
    })
}

async fn handle_key(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    key: KeyEvent,
) -> Result<()> {
    if key.kind != KeyEventKind::Press {
        return Ok(());
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            request_cancel(terminal, app).await?;
        }
        KeyCode::Esc => {
            request_cancel(terminal, app).await?;
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if app
                .shell_tx
                .send(approve_event(app, "all".to_string()))
                .await
                .is_err()
            {
                insert_log(terminal, "error", Color::Red, "kernel channel closed")?;
                app.status = "kernel disconnected".to_string();
            } else {
                app.status = "approval sent".to_string();
            }
        }
        KeyCode::Enter => submit_input(terminal, app).await?,
        KeyCode::Backspace => {
            if app.input_cursor > 0 {
                let remove_at = previous_char_boundary(&app.input, app.input_cursor);
                app.input.drain(remove_at..app.input_cursor);
                app.input_cursor = remove_at;
            }
        }
        KeyCode::Delete => {
            if app.input_cursor < app.input.len() {
                let end = next_char_boundary(&app.input, app.input_cursor);
                app.input.drain(app.input_cursor..end);
            }
        }
        KeyCode::Left => {
            if app.input_cursor > 0 {
                app.input_cursor = previous_char_boundary(&app.input, app.input_cursor);
            }
        }
        KeyCode::Right => {
            if app.input_cursor < app.input.len() {
                app.input_cursor = next_char_boundary(&app.input, app.input_cursor);
            }
        }
        KeyCode::Home => app.input_cursor = 0,
        KeyCode::End => app.input_cursor = app.input.len(),
        KeyCode::Char(ch) => {
            app.input.insert(app.input_cursor, ch);
            app.input_cursor += ch.len_utf8();
        }
        _ => {}
    }

    Ok(())
}

async fn request_cancel(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
) -> Result<()> {
    if app.shell_tx.send(cancel_event(app)).await.is_err() {
        insert_log(terminal, "error", Color::Red, "kernel channel closed")?;
        app.status = "kernel disconnected".to_string();
    } else {
        app.status = "cancel requested".to_string();
    }
    Ok(())
}

async fn submit_input(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
) -> Result<()> {
    let content = app.input.trim().to_string();
    if content.is_empty() {
        return Ok(());
    }

    app.input.clear();
    app.input_cursor = 0;
    app.status = "request sent".to_string();
    insert_log(terminal, "user", Color::Cyan, &content)?;

    if let Some(command) = content.strip_prefix('/') {
        submit_command(terminal, app, command.trim()).await?;
        return Ok(());
    }

    if !app.input_enabled {
        insert_log(
            terminal,
            "error",
            Color::Red,
            "input is disabled while the selected target is running",
        )?;
        app.status = "input disabled".to_string();
        return Ok(());
    }
    if app.run_uuid == DEFAULT_RUN_UUID || app.agent_uuid == DEFAULT_AGENT_UUID {
        insert_log(
            terminal,
            "error",
            Color::Red,
            "no active run; use /new <title> or /resume <run-uuid>",
        )?;
        app.status = "no active run".to_string();
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
        insert_log(terminal, "error", Color::Red, "kernel channel closed")?;
        app.status = "kernel disconnected".to_string();
    } else {
        app.input_enabled = false;
    }

    Ok(())
}

async fn submit_command(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    command: &str,
) -> Result<()> {
    let event = match command {
        "list" => command_event(UserKernelCommand::ListRuns),
        "cancel" => cancel_event(app),
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
                    Some(title.to_string())
                },
            })
        }
        cmd if cmd.starts_with("resume ") => {
            let run_uuid = cmd[7..].trim().to_string();
            command_event(UserKernelCommand::ResumeRun { run_uuid })
        }
        cmd if cmd.starts_with("delete ") => {
            let run_uuid = cmd[7..].trim().to_string();
            command_event(UserKernelCommand::DeleteRun { run_uuid })
        }
        cmd if cmd.starts_with("snapshot") => {
            let snapshot_uid = cmd
                .strip_prefix("snapshot")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            command_event(UserKernelCommand::Snapshot {
                run_uuid: None,
                snapshot_uid,
            })
        }
        _ => {
            insert_log(
                terminal,
                "error",
                Color::Red,
                &format!("unknown command: {command}"),
            )?;
            app.status = "unknown command".to_string();
            return Ok(());
        }
    };

    if app.shell_tx.send(event).await.is_err() {
        insert_log(terminal, "error", Color::Red, "kernel channel closed")?;
        app.status = "kernel disconnected".to_string();
    }
    Ok(())
}

fn drain_kernel_events(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
) -> Result<()> {
    while let Ok(event) = app.kernel_rx.try_recv() {
        match event {
            KernelToShellEvent::Snapshot { snapshot, .. } => {
                apply_snapshot(terminal, app, snapshot)?
            }
            KernelToShellEvent::Patch { text, .. } => {
                insert_log(terminal, "system", Color::Yellow, &text)?;
                app.status = text;
                app.input_enabled = true;
            }
        }
    }
    Ok(())
}

fn apply_snapshot(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    snapshot: KernelSnapshot,
) -> Result<()> {
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
        insert_units(terminal, &new_units)?;
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
    Ok(())
}

fn find_agent<'a>(snapshot: &'a KernelSnapshot, agent_uuid: &str) -> Option<&'a AgentSnapshot> {
    snapshot
        .agents
        .iter()
        .find(|agent| agent.agent.uuid == agent_uuid)
}

fn insert_units(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    units: &[&Unit],
) -> Result<()> {
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
            UnitRole::User => insert_log(terminal, "user", Color::Cyan, &content)?,
            UnitRole::Assistant => insert_log(terminal, "agent", Color::Green, &content)?,
            UnitRole::System => insert_log(terminal, "system", Color::Yellow, &content)?,
            UnitRole::Tool => insert_log(terminal, "tool", Color::Green, &content)?,
        }
    }
    Ok(())
}

fn insert_log(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    source: &'static str,
    color: Color,
    content: &str,
) -> Result<()> {
    let width = terminal.size()?.width as usize;
    let lines = log_lines(source, color, content, width);
    let height = lines.len().max(1) as u16;
    terminal.insert_before(height, |buf| render_lines(buf, lines))?;
    Ok(())
}

fn render_lines(buf: &mut Buffer, lines: Vec<Line<'static>>) {
    Paragraph::new(lines).render(buf.area, buf);
}

fn log_lines(
    source: &'static str,
    color: Color,
    content: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(8).max(1);
    let mut lines = Vec::new();
    for (line_index, line) in wrap_text(content, content_width).into_iter().enumerate() {
        let label = if line_index == 0 { source } else { "" };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label:<7}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::raw(line),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::raw(""));
    }
    lines
}

fn draw(frame: &mut Frame<'_>, app: &TuiApp) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    let status = Line::from(vec![
        Span::styled(
            "PrismAgent",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            " run={} agent={} input={} status={}",
            app.run_uuid,
            app.agent_uuid,
            if app.input_enabled { "on" } else { "off" },
            app.status
        )),
    ]);
    frame.render_widget(Paragraph::new(status), chunks[0]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    frame.render_widget(input, chunks[1]);

    let input_width = chunks[1].width.saturating_sub(2).max(1) as usize;
    let (cursor_x, cursor_y) = cursor_position(&app.input, app.input_cursor, input_width);
    frame.set_cursor_position(Position {
        x: chunks[1].x + 1 + cursor_x,
        y: chunks[1].y + 1 + cursor_y,
    });

    frame.render_widget(
        Paragraph::new("Enter send  Ctrl-Y approve all  Esc/Ctrl-C cancel  Ctrl-D quit"),
        chunks[2],
    );
}

fn wrap_text(content: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for raw_line in content.replace('\t', "    ").split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut current_width = 0usize;
        for ch in raw_line.chars() {
            let char_width = ch.width().unwrap_or(0);
            if current_width > 0 && current_width + char_width > width {
                lines.push(current);
                current = String::new();
                current_width = 0;
            }
            current.push(ch);
            current_width += char_width;
            if current_width >= width {
                lines.push(current);
                current = String::new();
                current_width = 0;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn cursor_position(content: &str, cursor: usize, width: usize) -> (u16, u16) {
    let before_cursor = &content[..cursor];
    let mut lines = wrap_text(before_cursor, width);
    if lines.is_empty() {
        lines.push(String::new());
    }
    let y = lines.len().saturating_sub(1);
    let x = lines
        .last()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .unwrap_or(0);
    (x as u16, y as u16)
}

fn previous_char_boundary(content: &str, cursor: usize) -> usize {
    content[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(content: &str, cursor: usize) -> usize {
    content[cursor..]
        .char_indices()
        .nth(1)
        .map(|(index, _)| cursor + index)
        .unwrap_or(content.len())
}
