# AGENTS.md

## Scope
These instructions apply to all files under `mini-swe-agent-rs/`.

## Project Goal
Build a Rust prototype/port of `mini-swe-agent` with:

- A minimal but usable Ratatui/Crossterm TUI.
- Explicit `CHAT` mode for conversations/questions only.
- Explicit `BUILD` mode for coding/tasks using mini-swe-agent methodology.
- CLI/non-interactive modes for testing and automation.
- Config/auth support for API key, base URL, and model name.
- Session persistence with resumable TUI sessions.

## Hard Requirements
- Use Rust.
- Use `ratatui` and `crossterm` for TUI.
- Keep the underlying agent/harness logic stable unless explicitly asked to change it.
- Prefer UX/QOL improvements over agent behavior changes.
- Keep `CHAT` and `BUILD` mode behavior distinct:
  - `CHAT`: no tools, conversation/question answering only.
  - `BUILD`: tool-enabled task execution.
- Keep a visible reminder in the TUI: conversations use CHAT; tasks/building use BUILD.

## Style Preferences
- Keep structure simple and non-overengineered.
- Prefer focused modules over large rewrites.
- Avoid unnecessary abstractions.
- Preserve the existing module split:
  - `src/main.rs`
  - `src/agent.rs`
  - `src/config.rs`
  - `src/environment.rs`
  - `src/markdown.rs`
  - `src/model.rs`
  - `src/prompts.rs`
  - `src/session.rs`
  - `src/tui.rs`
  - `src/types.rs`
- Do not add inline comments unless they materially improve clarity.
- Do not add new dependencies unless they clearly improve the implementation.

## TUI Preferences
- TUI should feel terminal-native and Codex-inspired.
- Transcript should remain scrollable.
- Full session history should be accessible by scrolling or loading saved sessions.
- Prompt/composer UX should be high quality and may use Codex as inspiration.
- Slash commands should be discoverable through popup menus.
- Session selection should use an interactive popup menu.
- Long observations should be collapsible/truncated.
- Assistant responses should support markdown rendering and streaming.

## Config
Config path defaults to:

```text
~/.config/mini-swe-agent-rs/config.json
```

Environment variables override config:

- `OPENAI_API_KEY`
- `OPENAI_BASE_URL`
- `MINI_SWE_MODEL`
- `MINI_SWE_CONFIG`
- `MINI_SWE_SESSIONS`

## Sessions
Sessions are stored under:

```text
~/.local/share/mini-swe-agent-rs/sessions/
```

or `MINI_SWE_SESSIONS` if set.

Session UX expectations:

- `/sessions` opens an interactive session menu.
- `/resume` opens the same session menu.
- `/load` opens the same session menu.
- `/load <id-prefix>` directly loads a session.
- `/latest` loads the latest session.
- `/new` starts a fresh session.
- `/save` saves the current session.

## Validation
After code changes, run:

```bash
cargo fmt && cargo check
```

Run from `mini-swe-agent-rs/`.

If tests are added later, run the most specific relevant tests first.

## Important Notes
- Do not commit changes unless explicitly requested.
- Do not rename files or reorganize modules unless explicitly requested.
- Do not copy large portions of Codex blindly; first check dependencies and integration cost.
- If adapting Codex TUI components, preserve this project's simpler architecture where possible.
