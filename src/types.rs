use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug)]
pub enum UiMsg {
    Log(Item),
    ChatDone(Vec<ChatMessage>),
    Done,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub title: String,
    pub body: String,
    pub color: Color,
    pub is_markdown: bool,
    pub is_truncatable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Chat,
    Build,
}

impl Mode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Chat => Self::Build,
            Self::Build => Self::Chat,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "CHAT",
            Self::Build => "BUILD",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

pub struct CommandOutput {
    pub output: String,
    pub returncode: i32,
    pub exception_info: String,
}
