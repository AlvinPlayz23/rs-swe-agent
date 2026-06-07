use crate::{
    agent::{run_agent, run_chat},
    config::Config,
    markdown::render_markdown,
    session::{
        latest_session, list_sessions, load_session, resolve_session_id, save_session, short_id,
        Session, SessionSummary,
    },
    types::{ChatMessage, Item, Mode, UiMsg},
};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
    Terminal,
};
use std::{collections::HashSet, io, time::Duration};
use tokio::sync::mpsc;

struct App {
    items: Vec<Item>,
    input: String,
    history: Vec<String>,
    history_pos: i64,
    running: bool,
    mode: Mode,
    chat_messages: Vec<ChatMessage>,
    scroll: usize,
    auto_scroll: bool,
    streaming_body: Option<String>,
    expanded_items: HashSet<usize>,
    model_name: String,
    session_id: String,
    session_created_at: u64,
    slash_selected: usize,
    thinking_tick: usize,
    session_menu: Option<SessionMenu>,
}

struct SessionMenu {
    sessions: Vec<SessionSummary>,
    selected: usize,
}

pub async fn run() -> Result<()> {
    run_with_session(None).await
}

pub async fn run_with_session(resume_id: Option<String>) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run_inner(&mut terminal, resume_id).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_inner(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    resume_id: Option<String>,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiMsg>();
    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<String>();

    let model_name = Config::load()
        .map(|config| config.model())
        .unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let loaded_session = match resume_id {
        Some(id) => Some(load_session(&resolve_session_id(&id).unwrap_or(id))?),
        None => None,
    };
    let new_session = Session::new(Mode::Chat, model_name.clone());

    let mut app = App {
        items: loaded_session.as_ref().map(Session::items).unwrap_or_else(|| vec![Item {
            title: "mini-swe-agent-rs".into(),
            body: "MODE: CHAT. Press Tab to switch modes. FOR CONVERSATIONS USE CHAT ONLY; FOR TASKS AND BUILDING USE BUILD MODE.".into(),
            color: Color::Yellow,
            is_markdown: false,
            is_truncatable: false,
        }]),
        input: String::new(),
        history: Vec::new(),
        history_pos: -1,
        running: false,
        mode: loaded_session.as_ref().map(Session::mode).unwrap_or(Mode::Chat),
        chat_messages: loaded_session
            .as_ref()
            .map(|s| s.chat_messages.clone())
            .unwrap_or_default(),
        scroll: 0,
        auto_scroll: true,
        streaming_body: None,
        expanded_items: HashSet::new(),
        model_name: loaded_session
            .as_ref()
            .map(|s| s.model.clone())
            .unwrap_or(model_name),
        session_id: loaded_session
            .as_ref()
            .map(|s| s.id.clone())
            .unwrap_or(new_session.id),
        session_created_at: loaded_session
            .as_ref()
            .map(|s| s.created_at)
            .unwrap_or(new_session.created_at),
        slash_selected: 0,
        thinking_tick: 0,
        session_menu: None,
    };

    loop {
        // Drain UI messages
        while let Ok(msg) = rx.try_recv() {
            let had_new = matches!(&msg, UiMsg::Log(_));
            match msg {
                UiMsg::Log(item) => {
                    app.streaming_body = None;
                    app.items.push(item);
                    autosave(&app);
                }
                UiMsg::ChatDone(messages) => {
                    app.chat_messages = messages;
                    app.running = false;
                    autosave(&app);
                }
                UiMsg::Done => {
                    app.running = false;
                    autosave(&app);
                }
            }
            if had_new && app.auto_scroll {
                app.scroll = 0;
            }
        }

        // Drain stream tokens and forward to UI
        while let Ok(token) = stream_rx.try_recv() {
            app.streaming_body
                .get_or_insert_with(String::new)
                .push_str(&token);
        }

        if app.running {
            app.thinking_tick = app.thinking_tick.wrapping_add(1);
        }

        terminal.draw(|f| draw(f, &mut app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) = event::read()?
            {
                // Only handle press/repeat, not release
                if kind == KeyEventKind::Release {
                    continue;
                }
                if app.session_menu.is_some() {
                    match code {
                        KeyCode::Esc => app.session_menu = None,
                        KeyCode::Up => {
                            if let Some(menu) = &mut app.session_menu {
                                menu.selected = menu.selected.saturating_sub(1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(menu) = &mut app.session_menu {
                                if !menu.sessions.is_empty() {
                                    menu.selected =
                                        (menu.selected + 1).min(menu.sessions.len() - 1);
                                }
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(menu) = &app.session_menu {
                                if let Some(summary) = menu.sessions.get(menu.selected) {
                                    match load_session(&summary.id) {
                                        Ok(session) => load_session_into_app(&mut app, session),
                                        Err(e) => push_error(&mut app, "load error", e),
                                    }
                                }
                            }
                            app.session_menu = None;
                            autosave(&app);
                        }
                        _ => {}
                    }
                    continue;
                }

                match (code, modifiers) {
                    (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Tab, _) if !app.running => {
                        app.mode = app.mode.toggle();
                        app.streaming_body = None;
                        app.items.push(Item {
                            title: "mode switched".into(),
                            body: format!("MODE: {}. FOR CONVERSATIONS USE CHAT ONLY; FOR TASKS AND BUILDING USE BUILD MODE.", app.mode.label()),
                            color: mode_color(app.mode),
                            is_markdown: false,
                            is_truncatable: false,
                        });
                        app.auto_scroll = true;
                        app.scroll = 0;
                        autosave(&app);
                    }
                    // Slash menu navigation
                    (KeyCode::Up, _) if !app.running && is_slash_menu_open(&app.input) => {
                        let count = slash_matches(&app.input).len();
                        if count > 0 {
                            app.slash_selected = app.slash_selected.saturating_sub(1);
                        }
                    }
                    (KeyCode::Down, _) if !app.running && is_slash_menu_open(&app.input) => {
                        let count = slash_matches(&app.input).len();
                        if count > 0 {
                            app.slash_selected = (app.slash_selected + 1).min(count - 1);
                        }
                    }
                    // Slash commands
                    (KeyCode::Enter, _) if !app.running && app.input.starts_with('/') => {
                        let cmd = resolve_slash_input(&app.input, app.slash_selected)
                            .unwrap_or_else(|| app.input.trim().to_lowercase());
                        let input = std::mem::take(&mut app.input);
                        app.items.push(Item {
                            title: "cmd".into(),
                            body: input,
                            color: Color::DarkGray,
                            is_markdown: false,
                            is_truncatable: false,
                        });
                        match cmd.as_str() {
                            "/clear" => {
                                let mode_hint = app.items.first().cloned();
                                app.items.clear();
                                if let Some(hint) = mode_hint {
                                    app.items.push(hint);
                                }
                                app.chat_messages.clear();
                            }
                            "/help" => {
                                app.items.push(Item {
                                    title: "help".into(),
                                    body: "CHAT MODE:\n  Ask questions, discuss code\n  No tools/commands\n\nBUILD MODE:\n  Mini-SWE-agent task execution\n  Bash tool for file editing, testing\n  Submit with: echo COMPLETE_TASK_AND_SUBMIT_FINAL_OUTPUT\n\nKEYS:\n  Tab          Switch CHAT/BUILD\n  Enter        Send message / Submit task\n  Shift+Enter  New line in input\n  Up/Down      Prompt history or slash menu\n  PgUp/PgDn    Scroll transcript\n  Ctrl+E       Toggle expand/collapse observation\n  /            Open slash command menu\n  /clear       Clear transcript\n  /help        This help\n  Esc/Ctrl+C   Quit".into(),
                                    color: Color::White,
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                            "/mode chat" => {
                                app.mode = Mode::Chat;
                                app.items.push(Item {
                                    title: "mode".into(),
                                    body: "Switched to CHAT. FOR CONVERSATIONS USE CHAT ONLY."
                                        .into(),
                                    color: mode_color(app.mode),
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                            "/mode build" => {
                                app.mode = Mode::Build;
                                app.items.push(Item {
                                    title: "mode".into(),
                                    body:
                                        "Switched to BUILD. FOR TASKS AND BUILDING USE BUILD MODE."
                                            .into(),
                                    color: mode_color(app.mode),
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                            "/model" => {
                                app.items.push(Item {
                                    title: "model".into(),
                                    body: app.model_name.clone(),
                                    color: Color::DarkGray,
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                            "/status" => {
                                app.items.push(Item {
                                    title: "status".into(),
                                    body: format!("mode: {}\nmodel: {}\nrunning: {}\nhistory items: {}\ntranscript items: {}", app.mode.label(), app.model_name, app.running, app.history.len(), app.items.len()),
                                    color: Color::White,
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                            "/config" => {
                                app.items.push(Item {
                                    title: "config".into(),
                                    body: "Use CLI config commands:\n  --config-show\n  --config-path\n  --config-set api_key <key>\n  --config-set base_url <url>\n  --config-set model <model>\n\nEnv overrides: OPENAI_API_KEY, OPENAI_BASE_URL, MINI_SWE_MODEL".into(),
                                    color: Color::White,
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                            "/diff" => {
                                app.items.push(Item {
                                    title: "diff".into(),
                                    body: "Diff rendering is planned next. For now, ask BUILD mode to run: git diff --stat && git diff".into(),
                                    color: Color::Yellow,
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                            "/save" => match save_current_session(&app) {
                                Ok(()) => app.items.push(Item {
                                    title: "session saved".into(),
                                    body: app.session_id.clone(),
                                    color: Color::Green,
                                    is_markdown: false,
                                    is_truncatable: false,
                                }),
                                Err(e) => app.items.push(Item {
                                    title: "session save error".into(),
                                    body: format!("{e:#}"),
                                    color: Color::Red,
                                    is_markdown: false,
                                    is_truncatable: false,
                                }),
                            },
                            "/sessions" | "/resume" => open_session_menu(&mut app),
                            "/latest" => match latest_session() {
                                Ok(Some(summary)) => match load_session(&summary.id) {
                                    Ok(session) => load_session_into_app(&mut app, session),
                                    Err(e) => push_error(&mut app, "load error", e),
                                },
                                Ok(None) => app.items.push(Item {
                                    title: "sessions".into(),
                                    body: "No saved sessions.".into(),
                                    color: Color::Yellow,
                                    is_markdown: false,
                                    is_truncatable: false,
                                }),
                                Err(e) => push_error(&mut app, "sessions error", e),
                            },
                            "/new" => {
                                let session = Session::new(app.mode, app.model_name.clone());
                                app.session_id = session.id;
                                app.session_created_at = session.created_at;
                                app.items = vec![Item {
                                    title: "new session".into(),
                                    body: format!("session {}", short_id(&app.session_id)),
                                    color: Color::Green,
                                    is_markdown: false,
                                    is_truncatable: false,
                                }];
                                app.chat_messages.clear();
                                app.expanded_items.clear();
                            }
                            "/load" => open_session_menu(&mut app),
                            other if other.starts_with("/load ") => {
                                let id = other.trim_start_matches("/load ").trim();
                                match resolve_session_id(id)
                                    .and_then(|full_id| load_session(&full_id))
                                {
                                    Ok(session) => load_session_into_app(&mut app, session),
                                    Err(e) => push_error(&mut app, "load error", e),
                                }
                            }
                            "/expand" | "/e" => {
                                // Expand all truncated items
                                for i in 0..app.items.len() {
                                    if app.items[i].is_truncatable {
                                        app.expanded_items.insert(i);
                                    }
                                }
                            }
                            "/collapse" | "/c" => {
                                app.expanded_items.clear();
                            }
                            other => {
                                app.items.push(Item {
                                    title: "unknown command".into(),
                                    body: format!("Unknown command: {other}. Type /help for available commands."),
                                    color: Color::Red,
                                    is_markdown: false,
                                    is_truncatable: false,
                                });
                            }
                        }
                        app.auto_scroll = true;
                        app.scroll = 0;
                        autosave(&app);
                    }
                    // Multi-line: Shift+Enter = newline
                    (KeyCode::Enter, KeyModifiers::SHIFT) if !app.running => {
                        app.input.push('\n');
                    }
                    // Submit
                    (KeyCode::Enter, _) if !app.running && !app.input.trim().is_empty() => {
                        let input = std::mem::take(&mut app.input);
                        app.history_pos = -1;
                        submit_input(input, &mut app, tx.clone(), stream_tx.clone()).await;
                    }
                    // Prompt history
                    (KeyCode::Up, _) if !app.running && !app.history.is_empty() => {
                        let new_pos = (app.history_pos + 1).min(app.history.len() as i64 - 1);
                        if new_pos != app.history_pos {
                            if app.history_pos < 0 {
                                // Save current draft
                                app.history.push(String::new());
                            }
                            app.history_pos = new_pos;
                            app.input =
                                app.history[app.history.len() - 1 - new_pos as usize].clone();
                        }
                    }
                    (KeyCode::Down, _) if !app.running && app.history_pos >= 0 => {
                        if app.history_pos == 0 {
                            app.history_pos = -1;
                            // Restore saved draft
                            if app.history.last().map(|s| s.is_empty()).unwrap_or(false) {
                                app.history.pop();
                            }
                            app.input.clear();
                        } else {
                            app.history_pos -= 1;
                            app.input = app.history
                                [app.history.len() - 1 - app.history_pos as usize]
                                .clone();
                        }
                    }
                    // Scroll
                    (KeyCode::PageUp, _) if !app.running => {
                        app.auto_scroll = false;
                        app.scroll = app.scroll.saturating_add(5);
                    }
                    (KeyCode::PageDown, _) if !app.running => {
                        app.scroll = app.scroll.saturating_sub(5);
                        if app.scroll == 0 {
                            app.auto_scroll = true;
                        }
                    }
                    // Toggle expand/collapse for truncated items
                    (KeyCode::Char('e'), KeyModifiers::CONTROL) if !app.running => {
                        // Find the last truncated item and toggle
                        for i in (0..app.items.len()).rev() {
                            if app.items[i].is_truncatable {
                                if app.expanded_items.contains(&i) {
                                    app.expanded_items.remove(&i);
                                } else {
                                    app.expanded_items.insert(i);
                                }
                                break;
                            }
                        }
                    }
                    (KeyCode::Backspace, _) if !app.running => {
                        app.input.pop();
                        app.slash_selected = 0;
                    }
                    (KeyCode::Char(ch), _) if !app.running => {
                        app.input.push(ch);
                        app.slash_selected = 0;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

async fn submit_input(
    input: String,
    app: &mut App,
    tx: mpsc::UnboundedSender<UiMsg>,
    stream_tx: mpsc::UnboundedSender<String>,
) {
    // Save to history (avoid duplicates)
    if app.history.last().map(|s| s.as_str()) != Some(&input) {
        app.history.push(input.clone());
    }
    if app.history.len() > 50 {
        app.history.remove(0);
    }
    app.running = true;
    app.auto_scroll = true;
    app.scroll = 0;
    app.streaming_body = None;

    let tx2 = tx.clone();
    let stx = stream_tx.clone();
    match app.mode {
        Mode::Chat => {
            let messages = app.chat_messages.clone();
            tokio::spawn(async move {
                if let Err(e) = run_chat(input, messages, tx2.clone(), Some(stx)).await {
                    tx2.send(UiMsg::Log(Item {
                        title: "error".into(),
                        body: format!("{e:#}"),
                        color: Color::Red,
                        is_markdown: false,
                        is_truncatable: false,
                    }))
                    .ok();
                    tx2.send(UiMsg::Done).ok();
                }
            });
        }
        Mode::Build => {
            tokio::spawn(async move {
                if let Err(e) = run_agent(input, tx2.clone(), Some(stx)).await {
                    tx2.send(UiMsg::Log(Item {
                        title: "error".into(),
                        body: format!("{e:#}"),
                        color: Color::Red,
                        is_markdown: false,
                        is_truncatable: false,
                    }))
                    .ok();
                    tx2.send(UiMsg::Done).ok();
                }
            });
        }
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn open_session_menu(app: &mut App) {
    match list_sessions() {
        Ok(sessions) if sessions.is_empty() => app.items.push(Item {
            title: "sessions".into(),
            body: "No saved sessions.".into(),
            color: Color::Yellow,
            is_markdown: false,
            is_truncatable: false,
        }),
        Ok(sessions) => {
            app.session_menu = Some(SessionMenu {
                sessions,
                selected: 0,
            });
        }
        Err(e) => push_error(app, "sessions error", e),
    }
}

fn save_current_session(app: &App) -> Result<()> {
    let session = Session::from_app(
        app.session_id.clone(),
        app.session_created_at,
        app.mode,
        app.model_name.clone(),
        &app.items,
        &app.chat_messages,
    );
    save_session(&session)
}

fn autosave(app: &App) {
    let _ = save_current_session(app);
}

fn load_session_into_app(app: &mut App, session: Session) {
    app.session_id = session.id.clone();
    app.session_created_at = session.created_at;
    app.mode = session.mode();
    app.model_name = session.model.clone();
    app.items = session.items();
    app.chat_messages = session.chat_messages;
    app.expanded_items.clear();
    app.scroll = 0;
    app.auto_scroll = true;
    app.items.push(Item {
        title: "session loaded".into(),
        body: format!("{}", session.id),
        color: Color::Green,
        is_markdown: false,
        is_truncatable: false,
    });
}

fn push_error(app: &mut App, title: &str, error: anyhow::Error) {
    app.items.push(Item {
        title: title.into(),
        body: format!("{error:#}"),
        color: Color::Red,
        is_markdown: false,
        is_truncatable: false,
    });
}

const fn mode_color(mode: Mode) -> Color {
    match mode {
        Mode::Chat => Color::Cyan,
        Mode::Build => Color::Yellow,
    }
}

#[derive(Clone, Copy)]
struct SlashCommand {
    command: &'static str,
    description: &'static str,
}

const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        command: "/help",
        description: "Show keybindings and modes",
    },
    SlashCommand {
        command: "/clear",
        description: "Clear transcript and chat context",
    },
    SlashCommand {
        command: "/mode chat",
        description: "Switch to CHAT mode",
    },
    SlashCommand {
        command: "/mode build",
        description: "Switch to BUILD mode",
    },
    SlashCommand {
        command: "/model",
        description: "Show selected model",
    },
    SlashCommand {
        command: "/status",
        description: "Show session status",
    },
    SlashCommand {
        command: "/config",
        description: "Show config help",
    },
    SlashCommand {
        command: "/diff",
        description: "Show diff guidance",
    },
    SlashCommand {
        command: "/save",
        description: "Save current session",
    },
    SlashCommand {
        command: "/sessions",
        description: "Open saved sessions menu",
    },
    SlashCommand {
        command: "/resume",
        description: "Open saved sessions menu",
    },
    SlashCommand {
        command: "/load",
        description: "Load by id prefix: /load abc123",
    },
    SlashCommand {
        command: "/latest",
        description: "Load latest session",
    },
    SlashCommand {
        command: "/new",
        description: "Start a new session",
    },
    SlashCommand {
        command: "/expand",
        description: "Expand all long outputs",
    },
    SlashCommand {
        command: "/collapse",
        description: "Collapse long outputs",
    },
];

fn is_slash_menu_open(input: &str) -> bool {
    input.starts_with('/') && !input.contains('\n')
}

fn slash_matches(input: &str) -> Vec<SlashCommand> {
    let needle = input.trim().to_lowercase();
    if needle == "/" {
        return SLASH_COMMANDS.to_vec();
    }
    SLASH_COMMANDS
        .iter()
        .copied()
        .filter(|cmd| cmd.command.starts_with(&needle))
        .collect()
}

fn resolve_slash_input(input: &str, selected: usize) -> Option<String> {
    let exact = input.trim().to_lowercase();
    if exact.starts_with("/load ") {
        return Some(exact);
    }
    if SLASH_COMMANDS.iter().any(|cmd| cmd.command == exact) {
        return Some(exact);
    }
    slash_matches(input)
        .get(selected)
        .map(|cmd| cmd.command.to_string())
}

fn thinking_label(tick: usize) -> &'static str {
    const FRAMES: &[&str] = &[
        "thinking ·  ",
        "thinking ·· ",
        "thinking ···",
        "thinking    ",
    ];
    FRAMES[(tick / 4) % FRAMES.len()]
}

fn render_item_lines(item: &Item, width: usize, expanded: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let wrap_w = width.max(20);

    lines.push(Line::from(vec![Span::styled(
        format!("{}", item.title),
        Style::default().fg(item.color).add_modifier(Modifier::BOLD),
    )]));

    if item.body.is_empty() {
        return lines;
    }

    // Render body
    if item.is_markdown {
        for l in render_markdown(&item.body) {
            let line_str = l.to_string();
            let wrapped = textwrap::wrap(&line_str, wrap_w);
            for w in wrapped {
                lines.push(Line::from(w.into_owned()));
            }
        }
    } else if item.is_truncatable && !expanded {
        // Collapsible output: show first 10 lines
        let line_count = item.body.lines().count();
        if line_count > 15 {
            let truncated: String = item.body.lines().take(10).collect::<Vec<_>>().join("\n");
            for l in textwrap::wrap(&truncated, wrap_w) {
                lines.push(Line::from(Span::styled(
                    l.into_owned(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::from(Span::styled(
                format!("… {} more lines (Ctrl+E to expand)", line_count - 10),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        } else {
            for l in textwrap::wrap(&item.body, wrap_w) {
                lines.push(Line::from(l.into_owned()));
            }
        }
    } else {
        for l in textwrap::wrap(&item.body, wrap_w) {
            lines.push(Line::from(l.into_owned()));
        }
    }

    lines.push(Line::from(""));
    lines
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.size();
    if area.height < 4 || area.width < 4 {
        return;
    }

    // Layout: status (1), gap (1), transcript, Codex-inspired composer.
    let composer_height = composer_height(&app.input, area.width);
    let prompt_area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(composer_height),
        area.width,
        composer_height,
    );
    let status_area = Rect::new(area.x, area.y, area.width, 1);
    let gap_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let transcript_area = Rect::new(
        area.x,
        area.y + 2,
        area.width,
        area.height.saturating_sub(2 + composer_height),
    );

    // Status line
    let mut status_spans = vec![
        Span::styled(
            format!(" {} ", app.mode.label()),
            Style::default()
                .fg(Color::Black)
                .bg(mode_color(app.mode))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Tab:switch  "),
        Span::styled(
            app.model_name.clone(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ),
        Span::raw(format!("  history:{}", app.history.len())),
    ];
    if app.running {
        status_spans.push(Span::styled(
            format!("  {}", thinking_label(app.thinking_tick)),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(status_spans)), status_area);

    // Gap separator
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        ))),
        gap_area,
    );

    // Transcript
    let transcript_width = transcript_area.width as usize;
    let mut all_lines: Vec<Line<'static>> = Vec::new();

    // Item index tracking for expand
    let mut item_idx = 0usize;
    for item in &app.items {
        let expanded = app.expanded_items.contains(&item_idx);
        let rendered = render_item_lines(item, transcript_width, expanded);
        all_lines.extend(rendered);
        item_idx += 1;
    }

    // Streaming/thinking body
    if let Some(ref stream_body) = app.streaming_body {
        if !stream_body.is_empty() {
            all_lines.push(Line::from(vec![Span::styled(
                "assistant (streaming)",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]));
            let wrapped = textwrap::wrap(stream_body, transcript_width.max(20));
            for w in wrapped {
                all_lines.push(Line::from(Span::styled(
                    w.into_owned(),
                    Style::default().fg(Color::Green),
                )));
            }
            all_lines.push(Line::from(Span::styled(
                "▌",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::SLOW_BLINK),
            )));
        }
    } else if app.running {
        all_lines.push(Line::from(vec![
            Span::styled(
                "assistant ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                thinking_label(app.thinking_tick),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    let total_lines = all_lines.len();
    let visible_lines = transcript_area.height as usize;

    // Clamp scroll
    if app.scroll >= total_lines.saturating_sub(visible_lines) {
        if total_lines > visible_lines {
            app.scroll = total_lines - visible_lines;
        } else {
            app.scroll = 0;
            app.auto_scroll = true;
        }
    }

    let start = total_lines.saturating_sub(visible_lines + app.scroll);
    let end = (start + visible_lines).min(total_lines);
    let visible: Vec<Line> = if start < total_lines {
        all_lines[start..end].to_vec()
    } else {
        Vec::new()
    };

    f.render_widget(Paragraph::new(visible), transcript_area);

    // Scroll indicator
    if app.scroll > 0 {
        let indicator = format!("  ↑ scroll {}  ", app.scroll);
        let indicator_width = indicator.len() as u16;
        if indicator_width < area.width {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    indicator,
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                ))),
                Rect::new(
                    area.x + area.width.saturating_sub(indicator_width),
                    gap_area.y,
                    indicator_width,
                    1,
                ),
            );
        }
    }

    draw_composer(f, prompt_area, app);

    if !app.running && is_slash_menu_open(&app.input) && app.session_menu.is_none() {
        draw_slash_popup(f, area, prompt_area, app);
    }

    if app.session_menu.is_some() {
        draw_session_menu(f, area, app);
    }

    // Cursor position
    if !app.running {
        let (cursor_x, cursor_y) = composer_cursor(prompt_area, &app.input);
        f.set_cursor(cursor_x, cursor_y);
    }
}

fn composer_height(input: &str, width: u16) -> u16 {
    let inner_width = width.saturating_sub(4).max(1) as usize;
    let visual_lines = input
        .split('\n')
        .map(|line| (visible_width(line).max(1) + inner_width - 1) / inner_width)
        .sum::<usize>()
        .max(1);
    (visual_lines as u16 + 2).clamp(3, 7)
}

fn draw_composer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    if area.height < 3 || area.width < 8 {
        return;
    }

    let border = if app.running {
        Color::DarkGray
    } else {
        mode_color(app.mode)
    };
    let inner_width = area.width.saturating_sub(4) as usize;
    let top_label = format!(" {} prompt ", app.mode.label());
    let top_fill = area.width.saturating_sub(top_label.len() as u16 + 2) as usize;

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("╭", Style::default().fg(border)),
        Span::styled(
            top_label,
            Style::default().fg(border).add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(top_fill), Style::default().fg(border)),
        Span::styled("╮", Style::default().fg(border)),
    ]));

    let mut body_lines = composer_body_lines(
        &app.input,
        inner_width,
        area.height.saturating_sub(2) as usize,
    );
    if body_lines.is_empty() {
        body_lines.push(String::new());
    }
    for (idx, body) in body_lines.into_iter().enumerate() {
        let prefix = if app.running {
            if idx == 0 {
                "… "
            } else {
                "  "
            }
        } else if idx == 0 {
            "> "
        } else {
            "  "
        };
        let content_width = area.width.saturating_sub(4) as usize;
        let shown = truncate_to_width(&body, content_width.saturating_sub(2));
        let pad = content_width.saturating_sub(2 + visible_width(&shown));
        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(border)),
            Span::styled(
                prefix,
                Style::default().fg(if app.running { Color::DarkGray } else { border }),
            ),
            Span::raw(shown),
            Span::raw(" ".repeat(pad)),
            Span::styled("│", Style::default().fg(border)),
        ]));
    }

    let hint = if app.running {
        " running · wait for completion "
    } else if is_slash_menu_open(&app.input) {
        " ↑↓ select · Enter run · Esc quit "
    } else {
        " Enter send · Shift+Enter newline · / commands · Tab mode "
    };
    let hint_fill = area.width.saturating_sub(hint.len() as u16 + 2) as usize;
    lines.push(Line::from(vec![
        Span::styled("╰", Style::default().fg(border)),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
        Span::styled("─".repeat(hint_fill), Style::default().fg(border)),
        Span::styled("╯", Style::default().fg(border)),
    ]));

    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines), area);
}

fn composer_body_lines(input: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    let max_content = width.saturating_sub(2).max(1);
    for logical in input.split('\n') {
        if logical.is_empty() {
            out.push(String::new());
        } else {
            let mut remaining = logical;
            while !remaining.is_empty() {
                let chunk = take_width(remaining, max_content);
                let len = chunk.len();
                out.push(chunk.to_string());
                remaining = &remaining[len..];
            }
        }
        if out.len() >= max_lines {
            break;
        }
    }
    out.truncate(max_lines);
    out
}

fn composer_cursor(area: Rect, input: &str) -> (u16, u16) {
    let inner_width = area.width.saturating_sub(4) as usize;
    let max_body_lines = area.height.saturating_sub(2) as usize;
    let body = composer_body_lines(input, inner_width, max_body_lines);
    let line_idx = body
        .len()
        .saturating_sub(1)
        .min(max_body_lines.saturating_sub(1));
    let col = body.last().map(|s| visible_width(s)).unwrap_or(0);
    (
        area.x + 3 + col.min(inner_width.saturating_sub(2)) as u16,
        area.y + 1 + line_idx as u16,
    )
}

fn truncate_to_width(s: &str, width: usize) -> String {
    if visible_width(s) <= width {
        return s.to_string();
    }
    let mut out = take_width(s, width.saturating_sub(1)).to_string();
    out.push('…');
    out
}

fn take_width(s: &str, width: usize) -> &str {
    if width == 0 {
        return "";
    }
    let mut end = 0;
    for (idx, ch) in s.char_indices() {
        if idx >= width {
            break;
        }
        end = idx + ch.len_utf8();
    }
    &s[..end.min(s.len())]
}

fn draw_session_menu(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let Some(menu) = &app.session_menu else {
        return;
    };
    let width = area.width.saturating_sub(4).min(88).max(36);
    let height = (menu.sessions.len().min(12) as u16 + 3).min(area.height.saturating_sub(2));
    if height < 4 {
        return;
    }
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);
    f.render_widget(Clear, popup_area);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            " sessions ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  Enter:load  Up/Down:select  Esc:close",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    for (idx, session) in menu.sessions.iter().take((height - 3) as usize).enumerate() {
        let selected = idx == menu.selected.min(menu.sessions.len().saturating_sub(1));
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let dim = if selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:<8}", short_id(&session.id)),
                style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {:<5}", session.mode), style),
            Span::styled(
                format!(
                    " {:<18}",
                    session.model.chars().take(18).collect::<String>()
                ),
                dim,
            ),
            Span::styled(format!(" {} items", session.item_count), dim),
        ]));
    }

    f.render_widget(Paragraph::new(lines), popup_area);
}

fn draw_slash_popup(f: &mut ratatui::Frame, area: Rect, prompt_area: Rect, app: &mut App) {
    let matches = slash_matches(&app.input);
    if matches.is_empty() {
        return;
    }

    let max_rows = matches.len().min(8) as u16;
    let width = area.width.min(58).max(24);
    let x = area.x;
    let y = prompt_area.y.saturating_sub(max_rows + 1);
    let popup_area = Rect::new(x, y, width, max_rows + 1);

    f.render_widget(Clear, popup_area);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        " slash commands ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));

    for (idx, cmd) in matches.iter().take(max_rows as usize).enumerate() {
        let selected = idx == app.slash_selected.min(matches.len().saturating_sub(1));
        let style = if selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let desc_style = if selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:<13}", cmd.command),
                style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {}", cmd.description), desc_style),
        ]));
    }

    f.render_widget(Paragraph::new(lines), popup_area);
}

fn visible_width(s: &str) -> usize {
    // Simple visible width: count chars excluding newlines for cursor positioning
    s.chars().filter(|&c| c != '\n').count()
}
