use crate::{config::Config, types::ChatMessage, types::ToolCall, types::ToolFunction};
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio::sync::mpsc;

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChatMessage,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
    #[allow(dead_code)]
    usage: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[allow(dead_code)]
    role: Option<String>,
    content: Option<String>,
    #[allow(dead_code)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: usize,
    id: Option<String>,
    #[allow(dead_code)]
    r#type: Option<String>,
    function: Option<StreamToolFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamToolFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Clone)]
pub struct Model {
    client: Client,
    base_url: String,
    api_key: String,
    pub model_name: String,
}

impl Model {
    pub fn from_env() -> Result<Self> {
        let config = Config::load()?;
        Ok(Self {
            client: Client::new(),
            base_url: config.base_url(),
            api_key: config.api_key()?,
            model_name: config.model(),
        })
    }

    pub async fn query_chat(&self, messages: &[ChatMessage]) -> Result<ChatMessage> {
        self.query(messages, false, None).await
    }

    pub async fn query_build(&self, messages: &[ChatMessage]) -> Result<ChatMessage> {
        self.query(messages, true, None).await
    }

    pub async fn query_chat_stream(
        &self,
        messages: &[ChatMessage],
        stream_tx: mpsc::UnboundedSender<String>,
    ) -> Result<ChatMessage> {
        self.query(messages, false, Some(stream_tx)).await
    }

    pub async fn query_build_stream(
        &self,
        messages: &[ChatMessage],
        stream_tx: mpsc::UnboundedSender<String>,
    ) -> Result<ChatMessage> {
        self.query(messages, true, Some(stream_tx)).await
    }

    async fn query(
        &self,
        messages: &[ChatMessage],
        enable_tools: bool,
        stream_tx: Option<mpsc::UnboundedSender<String>>,
    ) -> Result<ChatMessage> {
        let streaming = stream_tx.is_some();

        let tools = if enable_tools {
            Some(json!([{
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
            }]))
        } else {
            None
        };

        let mut body = json!({
            "model": self.model_name,
            "messages": messages,
        });
        if let Some(t) = &tools {
            body["tools"] = t.clone();
            body["tool_choice"] = json!("auto");
        }
        if streaming {
            body["stream"] = json!(true);
        }

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        if !streaming {
            let parsed: ChatResponse = resp.json().await?;
            return parsed
                .choices
                .into_iter()
                .next()
                .map(|c| c.message)
                .ok_or_else(|| anyhow!("model returned no choices"));
        }

        // Streaming mode
        let stream_tx = stream_tx.unwrap();
        let mut full_content = String::new();
        let mut role = String::new();
        let mut tool_call_builders: BTreeMap<usize, ToolCallBuilder> = BTreeMap::new();
        let mut _finish_reason: Option<String> = None;

        let mut stream = resp.bytes_stream();
        let mut leftover = Vec::new();

        while let Some(item) = stream.next().await {
            let chunk = item?;
            leftover.extend_from_slice(&chunk);

            // Process complete lines from leftover buffer
            while let Some(newline_pos) = leftover.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = leftover.drain(..=newline_pos).collect();
                let line_str = String::from_utf8_lossy(&line);
                let line_str = line_str.trim();
                if line_str.is_empty() {
                    continue;
                }
                if !line_str.starts_with("data: ") {
                    continue;
                }
                let data = &line_str[6..];
                if data == "[DONE]" {
                    break;
                }
                match serde_json::from_str::<StreamChunk>(data) {
                    Ok(chunk_data) => {
                        for choice in chunk_data.choices {
                            if choice.finish_reason.is_some() {
                                _finish_reason = choice.finish_reason;
                            }
                            let delta = choice.delta;
                            if delta.role.is_some() {
                                role = delta.role.unwrap_or_default();
                            }
                            if let Some(content) = delta.content {
                                if !content.is_empty() {
                                    full_content.push_str(&content);
                                    let _ = stream_tx.send(content);
                                }
                            }
                            if let Some(tool_calls) = delta.tool_calls {
                                for tc in tool_calls {
                                    let builder = tool_call_builders
                                        .entry(tc.index)
                                        .or_insert_with(ToolCallBuilder::new);
                                    if let Some(id) = tc.id {
                                        builder.id = Some(id);
                                    }
                                    if let Some(ref func) = tc.function {
                                        if let Some(ref name) = func.name {
                                            builder.name = Some(name.clone());
                                        }
                                        if let Some(ref args) = func.arguments {
                                            builder.arguments.push_str(args);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // Some models send non-standard lines; skip gracefully
                        eprintln!("stream parse error: {e} for line: {data:?}");
                    }
                }
            }
        }

        // Reconstruct the message
        let mut message = ChatMessage {
            role: if role.is_empty() {
                "assistant".to_string()
            } else {
                role
            },
            content: Some(Value::String(full_content)),
            tool_call_id: None,
            tool_calls: None,
        };

        if !tool_call_builders.is_empty() {
            let mut tool_calls = Vec::new();
            for (_idx, builder) in tool_call_builders {
                if let (Some(id), Some(name)) = (builder.id, builder.name) {
                    tool_calls.push(ToolCall {
                        id,
                        kind: "function".to_string(),
                        function: ToolFunction {
                            name,
                            arguments: builder.arguments,
                        },
                    });
                }
            }
            if !tool_calls.is_empty() {
                message.tool_calls = Some(tool_calls);
            }
        }

        Ok(message)
    }
}

struct ToolCallBuilder {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl ToolCallBuilder {
    fn new() -> Self {
        Self {
            id: None,
            name: None,
            arguments: String::new(),
        }
    }
}
