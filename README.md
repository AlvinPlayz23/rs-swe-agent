# mini-swe-agent-rs

A small Rust port/prototype of `mini-swe-agent` with a minimal Ratatui/Crossterm TUI.

It keeps the core mini-SWE idea:

- linear history
- one tool: `bash`
- every bash action runs in an independent subprocess
- `echo COMPLETE_TASK_AND_SUBMIT_FINAL_OUTPUT` ends the build task

## Modes

The TUI has two modes (switch with **Tab**):

| Mode | Use | Behavior |
|------|-----|----------|
| **CHAT** | Conversation, questions, explanations | No tools; pure chat with persistent history |
| **BUILD** | Coding tasks, repo inspection, file editing | Bash tool enabled; mini-swe-agent task loop |

- **Enter** to send
- **Tab** to toggle mode
- **Esc** or **Ctrl-C** to quit

## Run (interactive TUI)

```bash
cd mini-swe-agent-rs
cargo run
```

## Run (non-interactive / one-shot)

One-shot chat/ask:

```bash
cargo run -- --chat "hello, explain this project"
cargo run -- --ask "how does ratatui work?"
```

One-shot build/task:

```bash
cargo run -- --build "inspect this project and rate the code quality"
cargo run -- --task "fix all compiler warnings"
```

## Configuration

Config is stored in `~/.config/mini-swe-agent-rs/config.json`.

Set values once:

```bash
cargo run -- --config-set api_key sk-openai-...

# optional overrides
cargo run -- --config-set base_url https://api.openai.com/v1
cargo run -- --config-set model gpt-4o-mini
```

Show config:

```bash
cargo run -- --config-show
```

Show config file path:

```bash
cargo run -- --config-path
```

Environment variables override config at runtime:

```text
OPENAI_API_KEY
OPENAI_BASE_URL
MINI_SWE_MODEL
```

## CLI help

```bash
mini-swe-agent-rs                          Start TUI
mini-swe-agent-rs --chat <message>         One-shot chat/ask mode
mini-swe-agent-rs --build <task>           One-shot build/task mode
mini-swe-agent-rs --config-set api_key <key>
mini-swe-agent-rs --config-set base_url <url>
mini-swe-agent-rs --config-set model <model>
mini-swe-agent-rs --config-show
mini-swe-agent-rs --config-path
mini-swe-agent-rs --help
```
