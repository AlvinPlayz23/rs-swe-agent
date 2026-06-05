use crate::{
    agent::{run_agent, run_chat},
    types::{ChatMessage, Item, Mode, UiMsg},
};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{io, time::Duration};
use tokio::sync::mpsc;

struct App {
    items: Vec<Item>,
    input: String,
    running: bool,
    mode: Mode,
    chat_messages: Vec<ChatMessage>,
}

pub async fn run() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = run_inner(&mut terminal).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run_inner(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiMsg>();
    let mut app = App {
        items: vec![Item {
            title: "mini-swe-agent-rs".into(),
            body: "MODE: CHAT. Press Tab to switch modes. FOR CONVERSATIONS USE CHAT ONLY; FOR TASKS AND BUILDING USE BUILD MODE/TASK MODE.".into(),
            color: Color::Yellow,
        }],
        input: String::new(),
        running: false,
        mode: Mode::Chat,
        chat_messages: Vec::new(),
    };

    loop {
        while let Ok(msg) = rx.try_recv() {
            match msg {
                UiMsg::Log(item) => app.items.push(item),
                UiMsg::ChatDone(messages) => {
                    app.chat_messages = messages;
                    app.running = false;
                }
                UiMsg::Done => app.running = false,
            }
        }

        terminal.draw(|f| draw(f, &app))?;

        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(KeyEvent {
                code, modifiers, ..
            }) = event::read()?
            {
                match (code, modifiers) {
                    (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Tab, _) if !app.running => {
                        app.mode = app.mode.toggle();
                        app.items.push(Item {
                            title: "mode switched".into(),
                            body: format!("MODE: {}. {}", app.mode.label(), mode_note(app.mode)),
                            color: mode_color(app.mode),
                        });
                    }
                    (KeyCode::Enter, _) if !app.running && !app.input.trim().is_empty() => {
                        let input = std::mem::take(&mut app.input);
                        app.running = true;
                        let tx2 = tx.clone();
                        match app.mode {
                            Mode::Chat => {
                                let messages = app.chat_messages.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = run_chat(input, messages, tx2.clone()).await {
                                        tx2.send(UiMsg::Log(Item {
                                            title: "error".into(),
                                            body: format!("{e:#}"),
                                            color: Color::Red,
                                        }))
                                        .ok();
                                        tx2.send(UiMsg::Done).ok();
                                    }
                                });
                            }
                            Mode::Build => {
                                tokio::spawn(async move {
                                    if let Err(e) = run_agent(input, tx2.clone()).await {
                                        tx2.send(UiMsg::Log(Item {
                                            title: "error".into(),
                                            body: format!("{e:#}"),
                                            color: Color::Red,
                                        }))
                                        .ok();
                                        tx2.send(UiMsg::Done).ok();
                                    }
                                });
                            }
                        }
                    }
                    (KeyCode::Backspace, _) if !app.running => {
                        app.input.pop();
                    }
                    (KeyCode::Char(ch), _) if !app.running => app.input.push(ch),
                    _ => {}
                }
            }
        }
    }

    Ok(())
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

const fn mode_color(mode: Mode) -> Color {
    match mode {
        Mode::Chat => Color::Cyan,
        Mode::Build => Color::Yellow,
    }
}

const fn mode_note(mode: Mode) -> &'static str {
    match mode {
        Mode::Chat => "FOR CONVERSATIONS USE CHAT ONLY.",
        Mode::Build => "FOR TASKS AND BUILDING USE BUILD MODE/TASK MODE.",
    }
}

fn draw(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(f.size());

    let status = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" MODE: {} ", app.mode.label()),
            Style::default()
                .fg(Color::Black)
                .bg(mode_color(app.mode))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  Tab: switch mode  |  Enter: send  |  Esc/Ctrl-C: quit  "),
        Span::styled(
            mode_note(app.mode),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("mode"));
    f.render_widget(status, chunks[0]);

    let mut lines = Vec::new();
    let width = chunks[1].width.saturating_sub(4) as usize;
    for item in &app.items {
        lines.push(Line::from(vec![Span::styled(
            item.title.clone(),
            Style::default().fg(item.color).add_modifier(Modifier::BOLD),
        )]));
        for l in textwrap::wrap(&item.body, width.max(20)) {
            lines.push(Line::from(l.into_owned()));
        }
        lines.push(Line::from(""));
    }

    let visible = chunks[1].height.saturating_sub(2) as usize;
    let start = lines.len().saturating_sub(visible);
    let log = Paragraph::new(lines[start..].to_vec())
        .block(Block::default().borders(Borders::ALL).title("conversation"))
        .wrap(Wrap { trim: false });
    f.render_widget(log, chunks[1]);

    let prompt_title = if app.running {
        format!("{} running...", app.mode.label())
    } else {
        format!("{} prompt", app.mode.label())
    };
    let prompt = Paragraph::new(app.input.as_str())
        .block(Block::default().borders(Borders::ALL).title(prompt_title));
    f.render_widget(prompt, chunks[2]);
}
