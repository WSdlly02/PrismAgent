use anyhow::{Result, anyhow};
use std::{io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use prismagent::{
    bus::{Bus, Subsystem, SubsystemName},
    subsystems::{
        agent_subsystem::model::AgentSubsystem,
        shell_subsystem::model::ShellSubsystem,
        shell_subsystem::model::{ShellAgentSnapshot, ShellMessage},
        shell_subsystem::model::{
            ShellApproveRequest, ShellEvent, ShellSnapshot, ShellSubmitRequest,
        },
    },
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
use serde_json::json;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const INLINE_HEIGHT: u16 = 7;

#[tokio::main]
async fn main() -> Result<()> {
    let bus = start_mock_runtime().await;
    let mut app = TuiApp::new(bus);

    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;
    insert_log(
        &mut terminal,
        "system",
        Color::Yellow,
        "PrismAgent mock runtime started. Type text to echo, /approve [args], Ctrl-D to quit.",
    )?;

    match request_snapshot(&app.bus).await {
        Ok(event) => apply_shell_event(&mut terminal, &mut app, event)?,
        Err(error) => insert_log(&mut terminal, "error", Color::Red, &error.to_string())?,
    }

    run_loop(&mut terminal, &mut app).await
}

async fn start_mock_runtime() -> Bus {
    let bus = Bus::new();

    let agent = AgentSubsystem::mock();
    let agent_tx = agent.start(bus.clone());
    bus.register(SubsystemName::Agent, agent_tx).await;

    let shell = ShellSubsystem::new();
    let shell_tx = shell.start(bus.clone());
    bus.register(SubsystemName::Shell, shell_tx).await;

    bus
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => handle_key(terminal, app, key).await?,
                Event::Paste(text) => insert_input(app, &text),
                Event::Resize(_, _) => terminal.autoresize()?,
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
    bus: Bus,
    agent_uuid: String,
    input: String,
    input_cursor: usize,
    input_enabled: bool,
    snapshot: Option<ShellSnapshot>,
    status: String,
    should_quit: bool,
}

impl TuiApp {
    fn new(bus: Bus) -> Self {
        Self {
            bus,
            agent_uuid: String::new(),
            input: String::new(),
            input_cursor: 0,
            input_enabled: true,
            snapshot: None,
            status: "idle".to_string(),
            should_quit: false,
        }
    }
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
        KeyCode::Esc => {
            insert_log(
                terminal,
                "system",
                Color::Yellow,
                "cancel is not wired in mock runtime",
            )?;
            app.status = "cancel unavailable".to_string();
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            insert_log(
                terminal,
                "system",
                Color::Yellow,
                "cancel is not wired in mock runtime",
            )?;
            app.status = "cancel unavailable".to_string();
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            approve(terminal, app, "all").await?;
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
        KeyCode::Char(ch) => insert_input(app, &ch.to_string()),
        _ => {}
    }

    Ok(())
}

fn insert_input(app: &mut TuiApp, content: &str) {
    app.input.insert_str(app.input_cursor, content);
    app.input_cursor += content.len();
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
    app.input_enabled = false;
    app.status = "request sent".to_string();

    let event = request_submit(app, content).await;
    match event {
        Ok(event) => apply_shell_event(terminal, app, event)?,
        Err(error) => {
            insert_log(terminal, "error", Color::Red, &error.to_string())?;
            app.status = "request failed".to_string();
            app.input_enabled = true;
        }
    }

    Ok(())
}

async fn approve(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    args: &str,
) -> Result<()> {
    let response = app
        .bus
        .post(
            SubsystemName::Shell,
            SubsystemName::Shell,
            "approve",
            json!(ShellApproveRequest {
                args: args.to_string(),
                agent_uuid: active_agent_uuid(app),
            }),
        )
        .await
        .map_err(|error| anyhow!("shell approve failed: {error}"))?;
    let event = shell_event_from_response(response)?;
    apply_shell_event(terminal, app, event)
}

async fn request_snapshot(bus: &Bus) -> Result<ShellEvent> {
    let response = bus
        .get(SubsystemName::Shell, SubsystemName::Shell, "snapshot")
        .await
        .map_err(|error| anyhow!("shell snapshot failed: {error}"))?;
    shell_event_from_response(response)
}

async fn request_submit(app: &TuiApp, content: String) -> Result<ShellEvent> {
    let response = app
        .bus
        .post(
            SubsystemName::Shell,
            SubsystemName::Shell,
            "submit",
            json!(ShellSubmitRequest {
                content,
                agent_uuid: active_agent_uuid(app),
            }),
        )
        .await
        .map_err(|error| anyhow!("shell submit failed: {error}"))?;
    shell_event_from_response(response)
}

fn shell_event_from_response(response: prismagent::bus::Response) -> Result<ShellEvent> {
    if !response.is_ok() {
        return Err(anyhow!("shell request failed: {:?}", response.body));
    }
    serde_json::from_value(response.body).map_err(Into::into)
}

fn active_agent_uuid(app: &TuiApp) -> Option<String> {
    if app.agent_uuid.is_empty() {
        None
    } else {
        Some(app.agent_uuid.clone())
    }
}

fn apply_shell_event(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    event: ShellEvent,
) -> Result<()> {
    match event {
        ShellEvent::Patch { text, .. } => {
            if !text.is_empty() {
                insert_log(terminal, "system", Color::Yellow, &text)?;
            }
            app.status = if text.is_empty() {
                "idle".to_string()
            } else {
                text
            };
            app.input_enabled = true;
        }
        ShellEvent::Snapshot { snapshot, .. } => apply_snapshot(terminal, app, snapshot)?,
    }
    Ok(())
}

fn apply_snapshot(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    snapshot: ShellSnapshot,
) -> Result<()> {
    let active_agent_uuid = snapshot.active_agent_uuid.clone();
    app.agent_uuid = active_agent_uuid.clone();

    let old_count = app
        .snapshot
        .as_ref()
        .and_then(|old_snapshot| find_agent(old_snapshot, &active_agent_uuid))
        .map(|agent| agent.messages.len())
        .unwrap_or(0);

    if let Some(agent) = find_agent(&snapshot, &active_agent_uuid) {
        let new_messages = agent.messages.iter().skip(old_count).collect::<Vec<_>>();
        insert_messages(terminal, &new_messages)?;
        app.status = format!(
            "agent={} messages={}",
            active_agent_uuid,
            agent.messages.len()
        );
    } else {
        app.status = "no active agent".to_string();
    }

    app.input_enabled = true;
    app.snapshot = Some(snapshot);
    Ok(())
}

fn find_agent<'a>(snapshot: &'a ShellSnapshot, agent_uuid: &str) -> Option<&'a ShellAgentSnapshot> {
    snapshot
        .agents
        .iter()
        .find(|agent| agent.agent_uuid == agent_uuid)
}

fn insert_messages(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    messages: &[&ShellMessage],
) -> Result<()> {
    for message in messages {
        let (source, color) = match message.role.as_str() {
            "user" => ("user", Color::Cyan),
            "assistant" => ("agent", Color::Green),
            "tool" => ("tool", Color::Green),
            "system" => ("system", Color::Yellow),
            _ => ("other", Color::Magenta),
        };
        insert_log(terminal, source, color, &message.content)?;
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
            " agent={} input={} status={}",
            if app.agent_uuid.is_empty() {
                "none"
            } else {
                &app.agent_uuid
            },
            if app.input_enabled { "on" } else { "off" },
            app.status
        )),
    ]);
    frame.render_widget(Paragraph::new(status), chunks[0]);

    let input = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false });
    let input_width = chunks[1].width.saturating_sub(2).max(1) as usize;
    let input_height = chunks[1].height.saturating_sub(2).max(1);
    let (cursor_x, cursor_y) = cursor_position(&app.input, app.input_cursor, input_width);
    let scroll_y = input_scroll(cursor_y, input_height);
    let input = input.scroll((scroll_y, 0));
    frame.render_widget(input, chunks[1]);

    frame.set_cursor_position(Position {
        x: chunks[1].x + 1 + cursor_x,
        y: chunks[1].y + 1 + cursor_y.saturating_sub(scroll_y),
    });

    frame.render_widget(
        Paragraph::new("Enter send  /approve approve all  Ctrl-Y approve all  Ctrl-D quit"),
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
    let mut y = lines.len().saturating_sub(1);
    let mut x = lines
        .last()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .unwrap_or(0);
    if x >= width {
        y += 1;
        x = 0;
    }
    (x as u16, y as u16)
}

fn input_scroll(cursor_y: u16, input_height: u16) -> u16 {
    cursor_y.saturating_sub(input_height.saturating_sub(1))
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
