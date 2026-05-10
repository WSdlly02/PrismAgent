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

use prismagent::{
    model::event::{KernelEvent, ShellEvent},
    model::kernel::Kernel,
};
#[tokio::main]
async fn main() -> Result<()> {
    run().await
}

const DEFAULT_RUN_ID: &str = "run-dev";
const DEFAULT_AGENT_ID: &str = "0";

pub async fn run() -> Result<()> {
    let kernel = Kernel::new();
    let (shell_tx, kernel_rx) = kernel.run();
    let mut app = TuiApp::new(DEFAULT_RUN_ID, DEFAULT_AGENT_ID, shell_tx, kernel_rx);
    app.push_system("PrismAgent TUI started. Enter sends a request. Esc or Ctrl-C exits.");

    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;

    let result = run_loop(&mut terminal, &mut app).await;
    let _ = app.shell_tx.send(ShellEvent::Shutdown).await;
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

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key).await?;
            }
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
    run_id: String,
    agent_id: String,
    input: String,
    status: String,
    lines: Vec<LogLine>,
    should_quit: bool,
    shell_tx: mpsc::Sender<ShellEvent>,
    kernel_rx: mpsc::Receiver<KernelEvent>,
}

impl TuiApp {
    fn new(
        run_id: &str,
        agent_id: &str,
        shell_tx: mpsc::Sender<ShellEvent>,
        kernel_rx: mpsc::Receiver<KernelEvent>,
    ) -> Self {
        Self {
            run_id: run_id.to_owned(),
            agent_id: agent_id.to_owned(),
            input: String::new(),
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
            if app
                .shell_tx
                .send(ShellEvent::UserInput {
                    run_id: app.run_id.clone(),
                    agent_id: app.agent_id.clone(),
                    content,
                })
                .await
                .is_err()
            {
                app.push_error("kernel channel closed");
                app.status = "kernel disconnected".to_owned();
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
            KernelEvent::Output {
                run_id,
                agent_id,
                content,
            } => {
                app.status = format!("output from {run_id}/{agent_id}");
                app.push_kernel(content.content);
            }
            KernelEvent::Error {
                run_id,
                agent_id,
                error,
            } => {
                app.status = format!("error from {run_id}/{agent_id}");
                if error.details.is_empty() {
                    app.push_error(error.error);
                } else {
                    app.push_error(format!("{}: {}", error.error, error.details));
                }
            }
            KernelEvent::Done { run_id, agent_id } => {
                app.status = format!("{run_id}/{agent_id} done");
            }
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
            "  run={} agent={}  status={}",
            app.run_id, app.agent_id, app.status
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
