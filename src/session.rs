use crate::types::{ChatMessage, Item, Mode};
use anyhow::{anyhow, Context, Result};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub mode: String,
    pub model: String,
    pub items: Vec<SessionItem>,
    pub chat_messages: Vec<ChatMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionItem {
    pub title: String,
    pub body: String,
    pub color: String,
    pub is_markdown: bool,
    pub is_truncatable: bool,
}

#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub id: String,
    pub updated_at: u64,
    pub mode: String,
    pub model: String,
    pub item_count: usize,
}

impl Session {
    pub fn new(mode: Mode, model: String) -> Self {
        let now = now_secs();
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: now,
            updated_at: now,
            mode: mode.label().to_string(),
            model,
            items: Vec::new(),
            chat_messages: Vec::new(),
        }
    }

    pub fn from_app(
        id: String,
        created_at: u64,
        mode: Mode,
        model: String,
        items: &[Item],
        chat_messages: &[ChatMessage],
    ) -> Self {
        Self {
            id,
            created_at,
            updated_at: now_secs(),
            mode: mode.label().to_string(),
            model,
            items: items.iter().map(SessionItem::from_item).collect(),
            chat_messages: chat_messages.to_vec(),
        }
    }

    pub fn mode(&self) -> Mode {
        match self.mode.as_str() {
            "BUILD" | "Build" | "build" => Mode::Build,
            _ => Mode::Chat,
        }
    }

    pub fn items(&self) -> Vec<Item> {
        self.items.iter().map(SessionItem::to_item).collect()
    }
}

impl SessionItem {
    fn from_item(item: &Item) -> Self {
        Self {
            title: item.title.clone(),
            body: item.body.clone(),
            color: color_name(item.color).to_string(),
            is_markdown: item.is_markdown,
            is_truncatable: item.is_truncatable,
        }
    }

    fn to_item(&self) -> Item {
        Item {
            title: self.title.clone(),
            body: self.body.clone(),
            color: color_from_name(&self.color),
            is_markdown: self.is_markdown,
            is_truncatable: self.is_truncatable,
        }
    }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn sessions_dir() -> Result<PathBuf> {
    if let Ok(path) = env::var("MINI_SWE_SESSIONS") {
        return Ok(PathBuf::from(path));
    }
    let home = env::var("HOME").context("HOME is not set; set MINI_SWE_SESSIONS")?;
    Ok(PathBuf::from(home).join(".local/share/mini-swe-agent-rs/sessions"))
}

pub fn session_path(id: &str) -> Result<PathBuf> {
    Ok(sessions_dir()?.join(format!("{id}.json")))
}

pub fn save_session(session: &Session) -> Result<()> {
    let dir = sessions_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = session_path(&session.id)?;
    fs::write(&path, serde_json::to_string_pretty(session)? + "\n")
        .with_context(|| format!("writing {}", path.display()))
}

pub fn load_session(id: &str) -> Result<Session> {
    let path = session_path(id)?;
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

pub fn latest_session() -> Result<Option<SessionSummary>> {
    let mut sessions = list_sessions()?;
    sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    Ok(sessions.into_iter().next())
}

pub fn list_sessions() -> Result<Vec<SessionSummary>> {
    let dir = sessions_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(session) = serde_json::from_str::<Session>(&text) else {
            continue;
        };
        out.push(SessionSummary {
            id: session.id,
            updated_at: session.updated_at,
            mode: session.mode,
            model: session.model,
            item_count: session.items.len(),
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    Ok(out)
}

pub fn format_sessions(sessions: &[SessionSummary]) -> String {
    if sessions.is_empty() {
        return "No saved sessions.".to_string();
    }
    sessions
        .iter()
        .take(20)
        .map(|s| {
            format!(
                "{}  {}  {:<5}  {:<18}  {} items",
                short_time(s.updated_at),
                short_id(&s.id),
                s.mode,
                trim_model(&s.model),
                s.item_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

pub fn resolve_session_id(prefix: &str) -> Result<String> {
    let sessions = list_sessions()?;
    let matches: Vec<_> = sessions
        .into_iter()
        .filter(|s| s.id.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [] => Err(anyhow!("no session matches {prefix:?}")),
        [one] => Ok(one.id.clone()),
        _ => Err(anyhow!(
            "multiple sessions match {prefix:?}; use a longer id"
        )),
    }
}

fn short_time(ts: u64) -> String {
    // Compact, dependency-free timestamp. Human-friendly enough for a TUI list.
    format!("t+{}", ts)
}

fn trim_model(model: &str) -> String {
    if model.len() <= 18 {
        model.to_string()
    } else {
        format!("{}…", &model[..17])
    }
}

fn color_name(color: Color) -> &'static str {
    match color {
        Color::Black => "black",
        Color::Red => "red",
        Color::Green => "green",
        Color::Yellow => "yellow",
        Color::Blue => "blue",
        Color::Magenta => "magenta",
        Color::Cyan => "cyan",
        Color::Gray | Color::White => "white",
        Color::DarkGray => "darkgray",
        Color::LightRed => "lightred",
        Color::LightGreen => "lightgreen",
        Color::LightYellow => "lightyellow",
        Color::LightBlue => "lightblue",
        Color::LightMagenta => "lightmagenta",
        Color::LightCyan => "lightcyan",
        _ => "white",
    }
}

fn color_from_name(name: &str) -> Color {
    match name {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "darkgray" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::White,
    }
}
