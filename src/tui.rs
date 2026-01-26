use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use crossbeam_channel::Receiver;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};

const TOP_PANE_HEIGHT: u16 = 7;

fn base_text_style() -> Style {
    // A calm (slightly lighter) blue-gray for primary text.
    Style::default().fg(Color::Rgb(185, 200, 212))
}

fn base_frame_style() -> Style {
    // Slightly darker than text for frames/borders.
    Style::default().fg(Color::Rgb(105, 120, 135))
}

fn base_block<T: Into<String>>(title: T) -> Block<'static> {
    Block::default()
        .title(title.into())
        .borders(Borders::ALL)
        .style(base_text_style())
        .border_style(base_frame_style())
}

#[derive(Debug, Clone)]
pub enum ActionState {
    Success,
    Failure,
    Running,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RepoStatusRow {
    pub name: String,
    pub action: ActionState,
    pub latest_release: Option<String>,
    pub ahead_by: Option<u32>,
    pub loading: bool,
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    SetStep { title: String, body: String },
    UpdateBody { body: String },
    SetOk { msg: String },
    SetError { msg: String },
    SetRepos { rows: Vec<RepoStatusRow> },
    Finished { ok: bool },
}

#[derive(Debug, Clone)]
enum Focus {
    Help,
    None,
}

#[derive(Debug, Clone)]
struct AppState {
    step_title: String,
    step_body: String,
    step_started_at: Instant,
    ok_msg: String,
    error_msg: Option<String>,
    repos: Vec<RepoStatusRow>,
    help_scroll: u16,
    focus: Focus,
    finished: Option<bool>,
}

impl AppState {
    fn new() -> Self {
        Self {
            step_title: "Initializing".to_string(),
            step_body: "Starting orchestrator…".to_string(),
            step_started_at: Instant::now(),
            ok_msg: "OK".to_string(),
            error_msg: None,
            repos: Vec::new(),
            help_scroll: 0,
            focus: Focus::None,
            finished: None,
        }
    }
}

const HELP_TEXT: &str = r#"Keys
  q / Esc       Quit
  Tab          Focus help
  Up/Down      Scroll help
  PgUp/PgDn    Scroll help faster

What you’re seeing
  Current Step: shows what the orchestrator is doing right now.
               This pane overwrites on each update (no scrolling spam).
  Status:       green OK when healthy; red ERROR when something fails.
    Completion:   stays open when done; press q to exit (or pass --auto-exit).
"#;

pub fn run(rx: Receiver<UiEvent>, auto_exit: bool) -> Result<()> {
    let mut stdout = std::io::stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut state = AppState::new();
    let tick = Duration::from_millis(60);
    let mut last_draw = Instant::now()
        .checked_sub(tick)
        .unwrap_or_else(Instant::now);

    let res = (|| -> Result<()> {
        loop {
            // Drain all pending UI events.
            while let Ok(ev) = rx.try_recv() {
                handle_ui_event(&mut state, ev);
            }

            // Keyboard input.
            if event::poll(Duration::from_millis(10))?
                && let Event::Key(key) = event::read()?
                && handle_key(&mut state, key)
            {
                break;
            }

            // If finished:
            // - On success: stay unless auto_exit is enabled.
            // - On error: always stay until user quits.
            if let Some(ok) = state.finished
                && ok
                && auto_exit
            {
                break;
            }

            if last_draw.elapsed() >= tick {
                terminal.draw(|f| ui(f, &state))?;
                last_draw = Instant::now();
            }
        }
        Ok(())
    })();

    // Restore terminal.
    terminal::disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    res
}

fn handle_ui_event(state: &mut AppState, ev: UiEvent) {
    match ev {
        UiEvent::SetStep { title, body } => {
            state.step_title = title;
            state.step_body = body;
            state.step_started_at = Instant::now();
        }
        UiEvent::UpdateBody { body } => {
            state.step_body = body;
        }
        UiEvent::SetOk { msg } => {
            state.error_msg = None;
            state.ok_msg = if msg.trim().is_empty() {
                "OK".to_string()
            } else {
                msg
            };
        }
        UiEvent::SetError { msg } => {
            state.error_msg = Some(msg);
        }
        UiEvent::SetRepos { rows } => {
            state.repos = rows;
        }
        UiEvent::Finished { ok } => {
            state.finished = Some(ok);
            if ok {
                state.error_msg = None;
                state.ok_msg = "DONE — press q to exit".to_string();
            }
        }
    }
}

fn handle_key(state: &mut AppState, key: KeyEvent) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => return true,
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        (KeyCode::Tab, _) => {
            state.focus = match state.focus {
                Focus::None => Focus::Help,
                Focus::Help => Focus::None,
            };
        }
        (KeyCode::Up, _) => {
            if matches!(state.focus, Focus::Help) {
                state.help_scroll = state.help_scroll.saturating_sub(1);
            }
        }
        (KeyCode::Down, _) => {
            if matches!(state.focus, Focus::Help) {
                state.help_scroll = state.help_scroll.saturating_add(1);
            }
        }
        (KeyCode::PageUp, _) => {
            if matches!(state.focus, Focus::Help) {
                state.help_scroll = state.help_scroll.saturating_sub(10);
            }
        }
        (KeyCode::PageDown, _) => {
            if matches!(state.focus, Focus::Help) {
                state.help_scroll = state.help_scroll.saturating_add(10);
            }
        }
        _ => {}
    }

    false
}

fn ui(f: &mut ratatui::Frame, state: &AppState) {
    let size = f.area();

    // Apply a consistent base style to the whole terminal area.
    f.render_widget(Block::default().style(base_text_style()), size);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(size);

    let left = cols[0];
    let right = cols[1];

    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(TOP_PANE_HEIGHT), Constraint::Min(0)])
        .split(left);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(TOP_PANE_HEIGHT), Constraint::Min(0)])
        .split(right);

    render_step(f, left_rows[0], state);
    render_repos(f, left_rows[1], state);
    render_status(f, right_rows[0], state);
    render_help(f, right_rows[1], state);
}

fn render_repos(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let block = base_block("Repos");

    let header = Row::new([
        Cell::from("Repo"),
        Cell::from("CI"),
        Cell::from("Latest Release"),
        Cell::from("Ahead"),
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let rows = state.repos.iter().map(|r| {
        let (ci_text, ci_style, release, ahead_txt, ahead_style) = if r.loading {
            (
                spinner_frame().to_string(),
                Style::default().fg(Color::DarkGray),
                "…".to_string(),
                "…".to_string(),
                Style::default().fg(Color::DarkGray),
            )
        } else {
            let (ci_text, ci_style) = match r.action {
                ActionState::Success => ("OK".to_string(), Style::default().fg(Color::Green)),
                ActionState::Failure => ("FAIL".to_string(), Style::default().fg(Color::Red)),
                ActionState::Running => ("RUN".to_string(), Style::default().fg(Color::Yellow)),
                ActionState::Unknown => ("-".to_string(), Style::default().fg(Color::DarkGray)),
            };

            let release = r.latest_release.clone().unwrap_or_else(|| "-".to_string());

            let (ahead_txt, ahead_style) = match r.ahead_by {
                Some(0) => ("0".to_string(), Style::default().fg(Color::Green)),
                Some(n) => (format!("+{n}"), Style::default().fg(Color::Yellow)),
                None => ("-".to_string(), Style::default().fg(Color::DarkGray)),
            };

            (ci_text, ci_style, release, ahead_txt, ahead_style)
        };

        Row::new([
            Cell::from(r.name.clone()),
            Cell::from(ci_text).style(ci_style.add_modifier(Modifier::BOLD)),
            Cell::from(release),
            Cell::from(ahead_txt).style(ahead_style),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(33),
            Constraint::Length(6),
            Constraint::Length(14),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(block)
    .column_spacing(2);

    f.render_widget(table, area);
}

fn spinner_frame() -> char {
    const FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let i = (ms / 80) as usize;
    FRAMES[i % FRAMES.len()]
}

fn render_step(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let elapsed = state.step_started_at.elapsed();
    let spinner = spinner_frame();
    let title = format!(
        "Current Step  {spinner}  {:02}:{:02}",
        elapsed.as_secs() / 60,
        elapsed.as_secs() % 60
    );

    let block = base_block(title);

    let mut lines = Vec::new();
    lines.push(Line::styled(
        state.step_title.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(""));
    for l in state.step_body.lines() {
        lines.push(Line::raw(l.to_string()));
    }

    let para = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn render_status(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let (title, style, body) = match &state.error_msg {
        Some(err) => (
            "Status".to_string(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            format!("ERROR\n{}", err),
        ),
        None => (
            "Status".to_string(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            state.ok_msg.clone(),
        ),
    };

    let block = base_block(title);
    let para = Paragraph::new(body)
        .block(block)
        .style(style)
        .wrap(Wrap { trim: true });
    f.render_widget(para, area);
}

fn render_help(f: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let focused = matches!(state.focus, Focus::Help);
    let title = if focused { "Help (focused)" } else { "Help" };
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        base_frame_style()
    };

    let block = base_block(title).border_style(border_style);

    let para = Paragraph::new(HELP_TEXT)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((state.help_scroll, 0));

    f.render_widget(para, area);
}
