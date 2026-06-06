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
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
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
    scroll: usize,
    auto_scroll: bool,
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
        scroll: 0,
        auto_scroll: true,
    };

    loop {
        while let Ok(msg) = rx.try_recv() {
            let had_new = matches!(&msg, UiMsg::Log(_));
            match msg {
                UiMsg::Log(item) => app.items.push(item),
                UiMsg::ChatDone(messages) => {
                    app.chat_messages = messages;
                    app.running = false;
                }
                UiMsg::Done => app.running = false,
            }
            if had_new && app.auto_scroll {
                app.scroll = 0;
            }
        }

        terminal.draw(|f| draw(f, &mut app))?;

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
                        app.auto_scroll = true;
                        app.scroll = 0;
                    }
                    (KeyCode::Enter, _) if !app.running && !app.input.trim().is_empty() => {
                        let input = std::mem::take(&mut app.input);
                        app.running = true;
                        app.auto_scroll = true;
                        app.scroll = 0;
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
                    (KeyCode::Up, _) | (KeyCode::PageUp, _) if !app.running => {
                        app.auto_scroll = false;
                        app.scroll = app.scroll.saturating_add(3);
                    }
                    (KeyCode::Down, _) | (KeyCode::PageDown, _) if !app.running => {
                        app.scroll = app.scroll.saturating_sub(1);
                        if app.scroll == 0 {
                            app.auto_scroll = true;
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

fn render_lines<'a>(items: &'a [Item], width: usize) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    for item in items {
        lines.push(Line::from(vec![Span::styled(
            format!("{}", item.title),
            Style::default().fg(item.color).add_modifier(Modifier::BOLD),
        )]));
        for l in textwrap::wrap(&item.body, width.max(20)) {
            lines.push(Line::from(l.into_owned()));
        }
        lines.push(Line::from(""));
    }
    lines
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.size();
    if area.height < 4 || area.width < 4 {
        return;
    }

    // layout: status (1 line), gap (1), transcript (remaining - 2), prompt (1)
    let prompt_area = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    let status_area = Rect::new(area.x, area.y, area.width, 1);
    let gap_area = Rect::new(area.x, area.y + 1, area.width, 1);
    let transcript_area = Rect::new(
        area.x,
        area.y + 2,
        area.width,
        area.height.saturating_sub(3),
    );

    // status line
    let status = Line::from(vec![
        Span::styled(
            format!(" {} ", app.mode.label()),
            Style::default()
                .fg(Color::Black)
                .bg(mode_color(app.mode))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" Tab:switch  "),
        Span::styled(
            mode_note(app.mode),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        ),
    ]);
    f.render_widget(Paragraph::new(status), status_area);

    // gap
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        ))),
        gap_area,
    );

    // transcript
    let transcript_width = transcript_area.width as usize;
    let all_lines = render_lines(&app.items, transcript_width);
    let total_lines = all_lines.len();

    // clamp scroll
    let visible_lines = transcript_area.height as usize;
    if app.scroll >= total_lines.saturating_sub(visible_lines) {
        if total_lines > visible_lines {
            app.scroll = total_lines - visible_lines;
        } else {
            app.scroll = 0;
            app.auto_scroll = true;
        }
    }

    let start = total_lines.saturating_sub(visible_lines + app.scroll);
    let end = start + visible_lines.min(total_lines - start);
    let visible: Vec<Line> = if start < total_lines {
        all_lines[start..end].to_vec()
    } else {
        Vec::new()
    };

    f.render_widget(Paragraph::new(visible), transcript_area);

    // prompt
    let prompt_prefix = if app.running {
        Span::styled("...", Style::default().fg(Color::DarkGray))
    } else {
        Span::styled("> ", Style::default().fg(mode_color(app.mode)))
    };
    let prompt_line = Line::from(vec![prompt_prefix, Span::raw(&app.input)]);
    f.render_widget(Paragraph::new(prompt_line), prompt_area);

    // cursor
    let cursor_x = if app.running {
        0
    } else {
        2 + app.input.len() as u16
    };
    let cursor_y = prompt_area.y;
    f.set_cursor(area.x + cursor_x, cursor_y);
}
