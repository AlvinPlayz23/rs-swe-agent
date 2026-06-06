mod agent;
mod config;
mod environment;
mod markdown;
mod model;
mod prompts;
mod tui;
mod types;

use anyhow::{anyhow, Result};
use config::{config_path, Config};
use ratatui::style::Color;
use tokio::sync::mpsc;
use types::{ChatMessage, Item, UiMsg};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => tui::run().await,
        [flag, rest @ ..] if flag == "--chat" || flag == "--ask" => {
            run_print_chat(join_args(rest)?).await
        }
        [flag, rest @ ..] if flag == "--build" || flag == "--task" => {
            run_print_build(join_args(rest)?).await
        }
        [flag, key, value] if flag == "--config-set" => config_set(key, value),
        [flag] if flag == "--config-path" => {
            println!("{}", config_path()?.display());
            Ok(())
        }
        [flag] if flag == "--config-show" => {
            let config = Config::load()?;
            println!("{}", serde_json::to_string_pretty(&config)?);
            Ok(())
        }
        [flag] if flag == "--help" || flag == "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            print_help();
            Err(anyhow!("unknown arguments"))
        }
    }
}

fn join_args(args: &[String]) -> Result<String> {
    if args.is_empty() {
        Err(anyhow!("missing prompt text"))
    } else {
        Ok(args.join(" "))
    }
}

fn config_set(key: &str, value: &str) -> Result<()> {
    let mut config = Config::load()?;
    config.set(key, value.to_string())?;
    config.save()?;
    println!("saved {key} to {}", config_path()?.display());
    Ok(())
}

async fn run_print_chat(input: String) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiMsg>();
    // No streaming for print mode (simpler)
    tokio::spawn(async move {
        if let Err(e) = agent::run_chat(input, Vec::<ChatMessage>::new(), tx.clone(), None).await {
            tx.send(UiMsg::Log(Item {
                title: "error".into(),
                body: format!("{e:#}"),
                color: Color::Red,
                is_markdown: false,
                is_truncatable: false,
            }))
            .ok();
            tx.send(UiMsg::Done).ok();
        }
    });
    print_events(&mut rx).await
}

async fn run_print_build(input: String) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<UiMsg>();
    tokio::spawn(async move {
        if let Err(e) = agent::run_agent(input, tx.clone(), None).await {
            tx.send(UiMsg::Log(Item {
                title: "error".into(),
                body: format!("{e:#}"),
                color: Color::Red,
                is_markdown: false,
                is_truncatable: false,
            }))
            .ok();
            tx.send(UiMsg::Done).ok();
        }
    });
    print_events(&mut rx).await
}

async fn print_events(rx: &mut mpsc::UnboundedReceiver<UiMsg>) -> Result<()> {
    while let Some(msg) = rx.recv().await {
        match msg {
            UiMsg::Log(item) => {
                println!("\n== {} ==\n{}", item.title, item.body);
            }
            UiMsg::ChatDone(_) | UiMsg::Done => break,
        }
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "mini-swe-agent-rs\n\n\
Usage:\n  mini-swe-agent-rs                         Start TUI\n  mini-swe-agent-rs --chat <message>        One-shot non-interactive chat/ask mode\n  mini-swe-agent-rs --build <task>          One-shot non-interactive build/task mode\n  mini-swe-agent-rs --config-set api_key <key>\n  mini-swe-agent-rs --config-set base_url <url>\n  mini-swe-agent-rs --config-set model <model>\n  mini-swe-agent-rs --config-show\n  mini-swe-agent-rs --config-path\n\nEnv vars still override config: OPENAI_API_KEY, OPENAI_BASE_URL, MINI_SWE_MODEL\n"
    );
}
