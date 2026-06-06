use crate::{
    environment::LocalEnvironment,
    model::Model,
    prompts::{render_build_prompt, BUILD_SYSTEM_PROMPT, CHAT_SYSTEM_PROMPT},
    types::{ChatMessage, CommandOutput, Item, UiMsg},
};
use anyhow::{anyhow, Result};
use ratatui::style::Color;
use serde_json::{json, Value};
use tokio::sync::mpsc;

fn content_string(msg: &ChatMessage) -> String {
    match &msg.content {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    }
}

fn observation(out: &CommandOutput) -> String {
    if out.output.len() <= 10_000 {
        json!({"returncode": out.returncode, "output": out.output, "exception_info": out.exception_info})
            .to_string()
    } else {
        let head: String = out.output.chars().take(5000).collect();
        let tail: String = out
            .output
            .chars()
            .rev()
            .take(5000)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        json!({"returncode": out.returncode, "output_head": head, "output_tail": tail, "warning": "Output too long.", "exception_info": out.exception_info})
            .to_string()
    }
}

async fn build_model_and_env() -> Result<(Model, LocalEnvironment)> {
    let model = Model::from_env()?;
    let env = LocalEnvironment::new()?;
    Ok((model, env))
}

fn build_system_user(system_prompt: &str, user_content: String) -> Vec<ChatMessage> {
    vec![
        ChatMessage {
            role: "system".into(),
            content: Some(Value::String(system_prompt.into())),
            tool_call_id: None,
            tool_calls: None,
        },
        ChatMessage {
            role: "user".into(),
            content: Some(Value::String(user_content)),
            tool_call_id: None,
            tool_calls: None,
        },
    ]
}

pub async fn run_chat(
    input: String,
    mut messages: Vec<ChatMessage>,
    tx: mpsc::UnboundedSender<UiMsg>,
    stream_tx: Option<mpsc::UnboundedSender<String>>,
) -> Result<()> {
    let model = Model::from_env()?;

    tx.send(UiMsg::Log(Item {
        title: "user".into(),
        body: input.clone(),
        color: Color::Cyan,
        is_markdown: false,
        is_truncatable: false,
    }))
    .ok();

    if messages.is_empty() {
        messages.push(ChatMessage {
            role: "system".into(),
            content: Some(Value::String(CHAT_SYSTEM_PROMPT.into())),
            tool_call_id: None,
            tool_calls: None,
        });
    }
    messages.push(ChatMessage {
        role: "user".into(),
        content: Some(Value::String(input)),
        tool_call_id: None,
        tool_calls: None,
    });

    let assistant = if let Some(stx) = stream_tx {
        model.query_chat_stream(&messages, stx).await?
    } else {
        model.query_chat(&messages).await?
    };

    let text = content_string(&assistant);
    if !text.trim().is_empty() {
        tx.send(UiMsg::Log(Item {
            title: "assistant".into(),
            body: text,
            color: Color::Green,
            is_markdown: true,
            is_truncatable: false,
        }))
        .ok();
    }
    messages.push(assistant);
    tx.send(UiMsg::ChatDone(messages)).ok();
    Ok(())
}

pub async fn run_agent(
    task: String,
    tx: mpsc::UnboundedSender<UiMsg>,
    stream_tx: Option<mpsc::UnboundedSender<String>>,
) -> Result<()> {
    let (model, env) = build_model_and_env().await?;
    let mut messages = build_system_user(BUILD_SYSTEM_PROMPT, render_build_prompt(&task));

    tx.send(UiMsg::Log(Item {
        title: "user".into(),
        body: task,
        color: Color::Cyan,
        is_markdown: false,
        is_truncatable: false,
    }))
    .ok();

    for _step in 1..=100 {
        let assistant = if let Some(stx) = stream_tx.clone() {
            model.query_build_stream(&messages, stx).await?
        } else {
            model.query_build(&messages).await?
        };

        let text = content_string(&assistant);
        if !text.trim().is_empty() {
            tx.send(UiMsg::Log(Item {
                title: "assistant".into(),
                body: text,
                color: Color::Green,
                is_markdown: true,
                is_truncatable: false,
            }))
            .ok();
        }
        let tool_calls = assistant.tool_calls.clone().unwrap_or_default();
        messages.push(assistant);
        if tool_calls.is_empty() {
            tx.send(UiMsg::Done).ok();
            return Ok(());
        }
        for call in tool_calls {
            if call.function.name != "bash" {
                continue;
            }
            let args: Value =
                serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));
            let cmd = args.get("command").and_then(Value::as_str).unwrap_or("");
            tx.send(UiMsg::Log(Item {
                title: "tool: bash".into(),
                body: cmd.to_string(),
                color: Color::Yellow,
                is_markdown: false,
                is_truncatable: false,
            }))
            .ok();
            let out = env.execute(cmd).await;
            let obs = observation(&out);
            tx.send(UiMsg::Log(Item {
                title: format!("observation rc={}", out.returncode),
                body: out.output.clone(),
                color: if out.returncode == 0 {
                    Color::White
                } else {
                    Color::LightRed
                },
                is_markdown: false,
                is_truncatable: true,
            }))
            .ok();
            if out.returncode == 0
                && out.output.trim_start().lines().next()
                    == Some("COMPLETE_TASK_AND_SUBMIT_FINAL_OUTPUT")
            {
                tx.send(UiMsg::Log(Item {
                    title: "exit: Submitted".into(),
                    body: out.output.lines().skip(1).collect::<Vec<_>>().join("\n"),
                    color: Color::Magenta,
                    is_markdown: false,
                    is_truncatable: false,
                }))
                .ok();
                tx.send(UiMsg::Done).ok();
                return Ok(());
            }
            messages.push(ChatMessage {
                role: "tool".into(),
                content: Some(Value::String(obs)),
                tool_call_id: Some(call.id),
                tool_calls: None,
            });
        }
    }
    Err(anyhow!("step limit exceeded"))
}
