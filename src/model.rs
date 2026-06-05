use crate::{config::Config, types::ChatMessage};
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Clone)]
pub struct Model {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl Model {
    pub fn from_env() -> Result<Self> {
        let config = Config::load()?;
        Ok(Self {
            client: Client::new(),
            base_url: config.base_url(),
            api_key: config.api_key()?,
            model: config.model(),
        })
    }

    pub async fn query_chat(&self, messages: &[ChatMessage]) -> Result<ChatMessage> {
        self.query(messages, false).await
    }

    pub async fn query_build(&self, messages: &[ChatMessage]) -> Result<ChatMessage> {
        self.query(messages, true).await
    }

    async fn query(&self, messages: &[ChatMessage], enable_tools: bool) -> Result<ChatMessage> {
        let tools = json!([{
            "type": "function",
            "function": {
                "name": "bash",
                "description": "Execute a bash command in a fresh subshell.",
                "parameters": {
                    "type": "object",
                    "properties": {"command": {"type": "string", "description": "Command to run"}},
                    "required": ["command"],
                    "additionalProperties": false
                }
            }
        }]);
        let mut body = json!({
            "model": self.model,
            "messages": messages,
        });
        if enable_tools {
            body["tools"] = tools;
            body["tool_choice"] = json!("auto");
        }
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let parsed: ChatResponse = resp.json().await?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .ok_or_else(|| anyhow!("model returned no choices"))
    }
}
